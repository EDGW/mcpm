//! Static shell and batch script interpretation for server launch discovery.

use super::super::ServerLaunch;

use super::{
    java,
    tokens::{self, Syntax},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Clone, Debug)]
struct State {
    cwd: PathBuf,
    cwd_known: bool,
    vars: HashMap<String, String>,
    directories: Vec<PathBuf>,
    scopes: Vec<HashMap<String, String>>,
    exported: HashSet<String>,
    shell_script: PathBuf,
    shell_directory: PathBuf,
}

/// Independent command evidence and diagnostics; one failure never erases earlier launches.
#[derive(Default, Debug)]
pub(super) struct ScriptEvidence {
    pub launches: Vec<ServerLaunch>,
    pub diagnostics: Vec<anyhow::Error>,
}

pub(super) fn read(root: &Path, path: &Path) -> ScriptEvidence {
    let mut state = State {
        cwd: root.to_owned(),
        cwd_known: true,
        vars: HashMap::new(),
        directories: vec![],
        scopes: vec![],
        exported: HashSet::new(),
        shell_script: path.to_owned(),
        shell_directory: root.to_owned(),
    };
    let mut evidence = ScriptEvidence::default();
    if let Err(error) = visit(path, &mut state, &mut HashSet::new(), &mut evidence) {
        evidence.diagnostics.push(error);
    }
    evidence
}

fn visit(
    path: &Path,
    state: &mut State,
    stack: &mut HashSet<PathBuf>,
    evidence: &mut ScriptEvidence,
) -> Result<bool> {
    tracing::debug!(?path, "reading server start script");
    let canonical =
        fs::canonicalize(path).with_context(|| format!("failed to resolve script {path:?}"))?;
    if stack.len() >= 32 || !stack.insert(canonical.clone()) {
        return Err(ScriptError::Cycle {
            path: path.to_owned(),
        }
        .into());
    }
    let syntax = match path.extension().and_then(|s| s.to_str()) {
        Some(s) if s.eq_ignore_ascii_case("bat") || s.eq_ignore_ascii_case("cmd") => Syntax::Batch,
        Some("sh") => Syntax::Shell,
        _ => {
            return Err(ScriptError::Extension {
                path: path.to_owned(),
            }
            .into());
        }
    };
    let input = fs::read_to_string(path).map_err(|source| ScriptError::Read {
        path: path.to_owned(),
        source,
    })?;
    let mut joined = String::new();
    let mut first_line = 1;
    let mut terminated = false;
    let mut blocks = Blocks::default();
    for (index, line) in input.trim_start_matches('\u{feff}').lines().enumerate() {
        if joined.is_empty() {
            first_line = index + 1;
        }
        let line = line.trim_end();
        let continuation = if syntax == Syntax::Batch { '^' } else { '\\' };
        let suffix_count = line
            .chars()
            .rev()
            .take_while(|ch| *ch == continuation)
            .count();
        if suffix_count % 2 == 1 {
            joined.push_str(&line[..line.len() - continuation.len_utf8()]);
            continue;
        }
        joined.push_str(line);
        let mut inspected = tokens::inspect(clean_line(&joined, syntax), syntax);
        let skipped_block = blocks.skip(&mut inspected.parts, syntax);
        let stop = if skipped_block || inspected.unsupported {
            invalidate(&inspected.parts, syntax, state, !skipped_block)
        } else {
            match statement(&joined, path, first_line, syntax, state, stack, evidence) {
                Ok(stop) => stop,
                Err(source) => {
                    let stop = invalidate(&inspected.parts, syntax, state, true);
                    let error = ScriptError::Statement {
                        path: path.to_owned(),
                        line: first_line,
                        source,
                    };
                    tracing::debug!(error = %format_args!("{error:#}"), "skipping unavailable script evidence");
                    evidence.diagnostics.push(error.into());
                    stop
                }
            }
        };
        joined.clear();
        if stop {
            terminated = true;
            break;
        }
    }
    if !joined.is_empty() {
        evidence.diagnostics.push(
            ScriptError::Continuation {
                path: path.to_owned(),
                line: first_line,
            }
            .into(),
        );
    }
    stack.remove(&canonical);
    Ok(terminated)
}

fn statement(
    line: &str,
    path: &Path,
    line_number: usize,
    syntax: Syntax,
    state: &mut State,
    stack: &mut HashSet<PathBuf>,
    evidence: &mut ScriptEvidence,
) -> Result<bool> {
    let line = line.trim();
    if line.is_empty() || (syntax == Syntax::Shell && line.starts_with('#')) {
        return Ok(false);
    }
    let line = if syntax == Syntax::Batch {
        line.strip_prefix('@').unwrap_or(line)
    } else {
        line
    };
    let lower = line.to_ascii_lowercase();
    if syntax == Syntax::Batch
        && (lower == "rem" || lower.starts_with("rem ") || lower.starts_with("::"))
    {
        return Ok(false);
    }
    let head = lower.split_whitespace().next().unwrap_or("");
    if matches!(
        (syntax, head),
        (Syntax::Shell, "echo" | "printf") | (Syntax::Batch, "echo" | "pause")
    ) {
        return Ok(false);
    }
    if syntax == Syntax::Batch && (head == "goto" || head.starts_with(':')) {
        return Ok(false);
    }
    if matches!(head, "exit" | "return") {
        return Ok(true);
    }
    if syntax == Syntax::Shell && line.contains("$0") {
        ensure!(
            state.cwd == state.shell_directory,
            "$0 after changing working directory cannot be resolved conservatively"
        );
    }
    let expansion_script = if syntax == Syntax::Shell {
        &state.shell_script
    } else {
        path
    };
    // Shell invocation arguments are empty; batch %* is handled by the lexer.
    let line = if syntax == Syntax::Shell {
        line.strip_suffix(" \"$@\"").unwrap_or(line)
    } else {
        line
    };
    // Batch's conventional set "NAME=value" quotes encompass the assignment.
    let assignment = if syntax == Syntax::Batch && lower.starts_with("set ") {
        let assignment = line[4..].trim();
        let assignment = assignment
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(assignment);
        Some(assignment)
    } else if syntax == Syntax::Shell {
        let assignment = line.strip_prefix("export ").unwrap_or(line);
        assignment
            .split_once('=')
            .filter(|(name, _)| valid_name(name))
            .map(|_| assignment)
    } else {
        None
    };
    if let Some(assignment) = assignment {
        let (name, value) = assignment
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("expected literal variable assignment"))?;
        ensure!(valid_name(name), "unsupported variable assignment");
        let values = tokens::split(value, syntax, &state.vars, expansion_script);
        let name = if syntax == Syntax::Batch {
            name.to_ascii_uppercase()
        } else {
            name.to_owned()
        };
        state.vars.remove(&name);
        let values = values?;
        ensure!(
            values.len() <= 1,
            "variable assignment needs one literal value"
        );
        state
            .vars
            .insert(name.clone(), values.first().cloned().unwrap_or_default());
        if syntax == Syntax::Shell && line.starts_with("export ") {
            state.exported.insert(name);
        }
        return Ok(false);
    }
    if syntax == Syntax::Shell && line.starts_with("export ") {
        let names = tokens::split(&line[7..], syntax, &state.vars, expansion_script)?;
        ensure!(
            !names.is_empty() && names.iter().all(|name| valid_name(name)),
            "unsupported export declaration"
        );
        state.exported.extend(names);
        return Ok(false);
    }
    let raw_head = line.split_whitespace().next().unwrap_or("");
    let binary = raw_head.rsplit(['/', '\\']).next().unwrap_or(raw_head);
    let extension = Path::new(raw_head)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if raw_head
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_./\\-".contains(ch))
        && !matches!(binary.to_ascii_lowercase().as_str(), "java" | "java.exe")
        && !matches!(
            head,
            "exec"
                | "cd"
                | "chdir"
                | "pushd"
                | "popd"
                | "source"
                | "."
                | "sh"
                | "bash"
                | "call"
                | "set"
                | "setlocal"
                | "endlocal"
                | "export"
                | "unset"
                | "eval"
                | "read"
                | "shift"
        )
        && !matches!(
            extension.to_ascii_lowercase().as_str(),
            "sh" | "bat" | "cmd"
        )
    {
        return Ok(false);
    }
    if matches!(head, "eval" | "read" | "shift" | "unset") {
        bail!("cannot extract state changes from {head}");
    }
    let mut words = tokens::split(line, syntax, &state.vars, expansion_script)?;
    if words.is_empty() {
        return Ok(false);
    }
    let exec = syntax == Syntax::Shell && words.first().is_some_and(|s| s == "exec");
    if exec {
        words.remove(0);
    }
    ensure!(!words.is_empty(), "empty exec");
    let command = if syntax == Syntax::Batch {
        words[0].to_ascii_lowercase()
    } else {
        words[0].clone()
    };
    if (syntax == Syntax::Batch && matches!(command.as_str(), "echo" | "pause"))
        || (syntax == Syntax::Shell && matches!(command.as_str(), "echo" | "printf"))
    {
        return Ok(exec);
    }
    if syntax == Syntax::Batch && command == "setlocal" {
        ensure!(words.len() == 1, "setlocal options are unsupported");
        state.scopes.push(state.vars.clone());
        return Ok(false);
    }
    if syntax == Syntax::Batch && command == "endlocal" {
        ensure!(words.len() == 1, "unsupported endlocal arguments");
        state.vars = state
            .scopes
            .pop()
            .ok_or_else(|| anyhow::anyhow!("unmatched endlocal"))?;
        return Ok(false);
    }
    if matches!(command.as_str(), "cd" | "chdir" | "pushd") {
        let args: Vec<_> = words[1..]
            .iter()
            .filter(|s| {
                !(syntax == Syntax::Batch && s.eq_ignore_ascii_case("/d")) && s.as_str() != "--"
            })
            .collect();
        ensure!(args.len() == 1, "directory change needs one literal path");
        let value = if syntax == Syntax::Batch {
            args[0].replace('\\', "/")
        } else {
            args[0].to_owned()
        };
        ensure!(
            state.cwd_known || Path::new(&value).is_absolute(),
            "script working directory is unknown"
        );
        let new = state.cwd.join(value);
        if command == "pushd" && state.cwd_known {
            state.directories.push(state.cwd.clone());
        }
        state.cwd = new;
        state.cwd_known = true;
        return Ok(false);
    }
    if command == "popd" {
        ensure!(words.len() == 1, "unsupported popd arguments");
        state.cwd = state
            .directories
            .pop()
            .ok_or_else(|| anyhow::anyhow!("unmatched popd"))?;
        state.cwd_known = true;
        return Ok(false);
    }
    if matches!(command.as_str(), "exit" | "return") {
        return Ok(true);
    }
    let binary = command.rsplit(['/', '\\']).next().unwrap();
    if matches!(binary, "java" | "java.exe") {
        ensure!(state.cwd_known, "script working directory is unknown");
        evidence.launches.push(java::parse(
            state.cwd.clone(),
            path.to_owned(),
            line_number,
            syntax,
            words[0].clone(),
            words[1..].to_vec(),
            state
                .vars
                .iter()
                .filter(|(name, _)| syntax == Syntax::Batch || state.exported.contains(*name))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )?);
        return Ok(false);
    }
    let reference = matches!(
        (syntax, command.as_str()),
        (Syntax::Shell, "source" | "." | "sh" | "bash") | (Syntax::Batch, "call")
    );
    let target = if reference {
        ensure!(
            words.len() == 2,
            "referenced scripts with arguments are unsupported"
        );
        &words[1]
    } else {
        let extension = Path::new(&words[0])
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if !matches!(
            extension.to_ascii_lowercase().as_str(),
            "sh" | "bat" | "cmd"
        ) {
            // External helper commands are irrelevant to in-process variables and cwd.
            return Ok(exec);
        }
        ensure!(
            words.len() == 1,
            "referenced scripts with arguments are unsupported"
        );
        &words[0]
    };
    let child = resolve(&state.cwd, target, syntax);
    let extension = child.extension().and_then(|s| s.to_str()).unwrap_or("");
    ensure!(
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "sh" | "bat" | "cmd"
        ),
        "unsupported command {command:?}"
    );
    // Shell executables do not search the current directory without ./ or a path.
    if syntax == Syntax::Shell && !reference {
        ensure!(
            target.contains('/'),
            "bare shell script command depends on PATH"
        );
    }
    let child_syntax = if extension.eq_ignore_ascii_case("sh") {
        Syntax::Shell
    } else {
        Syntax::Batch
    };
    ensure!(
        child_syntax == syntax,
        "cross-shell script references are unsupported"
    );
    if matches!(command.as_str(), "source" | "." | "call") {
        ensure!(!exec, "exec of shell builtins is unsupported");
        let terminated = visit(&child, state, stack, evidence)?;
        if syntax == Syntax::Shell {
            return Ok(terminated);
        }
    } else {
        let mut child_state = state.clone();
        if syntax == Syntax::Shell {
            child_state.shell_script = child.clone();
            child_state.shell_directory = state.cwd.clone();
            child_state
                .vars
                .retain(|name, _| child_state.exported.contains(name));
        }
        visit(&child, &mut child_state, stack, evidence)?;
    }
    Ok(exec || (syntax == Syntax::Batch && !reference))
}

/// Remove comments and the batch echo-suppression prefix before structural inspection.
fn clean_line(line: &str, syntax: Syntax) -> &str {
    let line = line.trim();
    if syntax == Syntax::Batch {
        let line = line.strip_prefix('@').unwrap_or(line);
        let head = line.split_whitespace().next().unwrap_or("");
        if head.eq_ignore_ascii_case("rem") || line.starts_with("::") {
            ""
        } else {
            line
        }
    } else {
        line
    }
}

/// Only enough control structure to skip complete unsupported blocks, never evaluate them.
#[derive(Default)]
struct Blocks {
    shell: Vec<(&'static str, bool)>,
    batch_depth: usize,
}
impl Blocks {
    fn skip(&mut self, parts: &mut [tokens::Part<'_>], syntax: Syntax) -> bool {
        use tokens::Part;
        if syntax == Syntax::Batch {
            let head = parts
                .first()
                .and_then(|part| match part {
                    Part::Word(word) => Some(*word),
                    _ => None,
                })
                .unwrap_or("");
            let conditional = head.eq_ignore_ascii_case("if");
            let starts = (conditional || head.eq_ignore_ascii_case("for"))
                && parts.iter().any(|part| matches!(part, Part::Operator("(")));
            let skip = self.batch_depth > 0 || starts || conditional;
            if skip {
                for part in parts {
                    match part {
                        Part::Operator("(") => self.batch_depth += 1,
                        Part::Operator(")") => {
                            self.batch_depth = self.batch_depth.saturating_sub(1)
                        }
                        _ => {}
                    }
                }
            }
            return skip;
        }
        let mut skip = !self.shell.is_empty();
        let mut head = true;
        let mut redirected = false;
        for part in parts {
            match part {
                part if part.is_redirection() => redirected = true,
                Part::Operator(operator) => {
                    if let Some(("esac", pattern)) = self.shell.last_mut() {
                        if *operator == ")" {
                            *pattern = false;
                        }
                        if *operator == ";;" {
                            *pattern = true;
                        }
                    }
                    head = true;
                }
                Part::Word(_) if redirected => redirected = false,
                Part::Word(word)
                    if self.shell.last() == Some(&("esac", true)) && *word != "esac" =>
                {
                    *word = "";
                }
                Part::Word(word) if head => {
                    head = false;
                    match *word {
                        "if" => {
                            self.shell.push(("fi", false));
                            skip = true;
                        }
                        "for" | "while" | "until" => {
                            self.shell.push(("done", false));
                            skip = true;
                        }
                        "case" => {
                            self.shell.push(("esac", true));
                            skip = true;
                        }
                        "fi" | "done" | "esac" => {
                            if self.shell.last().is_some_and(|(end, _)| end == word) {
                                self.shell.pop();
                            }
                            skip = true;
                        }
                        "then" | "do" | "else" | "elif" => {
                            head = true;
                            skip = true;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        skip
    }
}

/// Discard only state that a skipped command might modify; never interpret its arguments.
fn invalidate(parts: &[tokens::Part<'_>], syntax: Syntax, state: &mut State, exits: bool) -> bool {
    use tokens::Part;
    let mut begin = 0;
    let mut conditional = false;
    let mut stop = false;
    let mut depth = 0usize;
    for index in 0..=parts.len() {
        if index == parts.len()
            || matches!(
                parts[index],
                Part::Operator(";" | ";;" | "&&" | "||" | "|" | "&" | "(" | ")")
            )
        {
            let words = &parts[begin..index];
            let asynchronous = matches!(parts.get(index), Some(Part::Operator("|" | "&")));
            stop |= invalidate_command(words, syntax, state)
                && exits
                && !conditional
                && depth == 0
                && !asynchronous;
            if let Some(Part::Operator(operator)) = parts.get(index) {
                match *operator {
                    ";" if depth == 0 => conditional = false,
                    "(" => {
                        depth += 1;
                    }
                    ")" => {
                        depth = depth.saturating_sub(1);
                    }
                    "&&" | "||" | "|" | "&" => conditional = true,
                    _ => {}
                }
            }
            begin = index + 1;
        }
    }
    stop
}

fn invalidate_command(parts: &[tokens::Part<'_>], syntax: Syntax, state: &mut State) -> bool {
    use tokens::Part;
    if parts.iter().any(tokens::Part::is_redirection) {
        let mut command = Vec::new();
        let mut parts = parts.iter();
        while let Some(part) = parts.next() {
            match part {
                part if part.is_redirection() => {
                    parts.next();
                }
                Part::Word(word) => command.push(Part::Word(word)),
                _ => {}
            }
        }
        return invalidate_command(&command, syntax, state);
    }
    let Some(Part::Word(raw)) = parts.first() else {
        return false;
    };
    // Decode only the command word, never the skipped launch's argument region.
    let decoded = tokens::split(raw, syntax, &state.vars, &state.shell_script).unwrap_or_default();
    let command = decoded.first().map(String::as_str).unwrap_or(raw);
    let command = if syntax == Syntax::Batch {
        command.to_ascii_lowercase()
    } else {
        command.to_owned()
    };
    let args = &parts[1..];
    if syntax == Syntax::Batch && command == "if" {
        return invalidate_batch_condition(args, state);
    }
    if matches!(
        command.as_str(),
        "then" | "do" | "else" | "elif" | "if" | "while" | "until" | "exec" | "!"
    ) {
        return invalidate_command(args, syntax, state);
    }
    if matches!(command.as_str(), "cd" | "chdir" | "pushd" | "popd") {
        state.cwd_known = false;
        if matches!(command.as_str(), "pushd" | "popd") {
            state.directories.clear();
        }
    }
    let extension = Path::new(command.as_str())
        .extension()
        .and_then(|s| s.to_str());
    if matches!(
        command.as_str(),
        "source" | "." | "call" | "sh" | "bash" | "eval"
    ) || matches!(extension, Some("sh" | "bat" | "cmd"))
    {
        state.cwd_known = false;
        state.directories.clear();
        state.vars.clear();
        state.scopes.clear();
    } else if matches!(command.as_str(), "setlocal" | "endlocal" | "read" | "shift") {
        state.vars.clear();
        state.scopes.clear();
    } else if matches!(command.as_str(), "export" | "unset" | "set" | "for") {
        if command == "set" && args.is_empty() {
            return false;
        }
        if syntax == Syntax::Batch
            && command == "set"
            && matches!(args.first(), Some(Part::Word("/a" | "/A")))
        {
            // Arithmetic may assign several names; discard involved identifiers without evaluating it.
            for part in &args[1..] {
                if let Part::Word(expression) = part {
                    for name in
                        expression.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                    {
                        forget_variable(name, syntax, state);
                    }
                }
            }
            return false;
        }
        let args = if syntax == Syntax::Batch
            && command == "set"
            && matches!(args.first(), Some(Part::Word("/p" | "/P" | "/a" | "/A")))
        {
            &args[1..]
        } else {
            args
        };
        for part in args {
            let Part::Word(word) = part else {
                continue;
            };
            let word = word.trim_matches(['\'', '"']);
            let name = word.split_once('=').map_or(word, |(name, _)| name);
            if command == "for" && matches!(name, "in" | "do") {
                break;
            }
            forget_variable(name, syntax, state);
            if command == "set" || command == "for" {
                break;
            }
        }
    } else if let Some((name, _)) = raw.split_once('=') {
        forget_variable(name, syntax, state);
        // Prefix assignments may be followed by a command with its own side effects.
        invalidate_command(args, syntax, state);
    }
    matches!(command.as_str(), "exit" | "return")
}

/// Locate the command after a batch IF condition without evaluating that condition.
fn invalidate_batch_condition(parts: &[tokens::Part<'_>], state: &mut State) -> bool {
    use tokens::Part;
    let mut parts = parts;
    while matches!(parts.first(), Some(Part::Word(word)) if word.eq_ignore_ascii_case("/i") || word.eq_ignore_ascii_case("not"))
    {
        parts = &parts[1..];
    }
    let count = match parts.first() {
        Some(Part::Word(word))
            if word.eq_ignore_ascii_case("exist")
                || word.eq_ignore_ascii_case("defined")
                || word.eq_ignore_ascii_case("errorlevel")
                || word.eq_ignore_ascii_case("cmdextversion") =>
        {
            2
        }
        Some(Part::Word(word)) if word.contains("==") => 1,
        Some(Part::Word(_)) if parts.len() >= 3 => 3,
        _ => return false,
    };
    if let Some(command) = parts.get(count..) {
        invalidate_command(command, Syntax::Batch, state);
    }
    false
}

fn forget_variable(name: &str, syntax: Syntax, state: &mut State) {
    if !valid_name(name) {
        return;
    }
    let name = if syntax == Syntax::Batch {
        name.to_ascii_uppercase()
    } else {
        name.to_owned()
    };
    state.vars.remove(&name);
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(super) fn resolve(cwd: &Path, value: &str, syntax: Syntax) -> PathBuf {
    cwd.join(if syntax == Syntax::Batch {
        value.replace('\\', "/")
    } else {
        value.to_owned()
    })
}

#[derive(Debug, Error)]
enum ScriptError {
    #[error("unsupported script extension: {path:?}; expected .sh, .bat, or .cmd")]
    Extension { path: PathBuf },
    #[error("unfinished script continuation in {path:?}:{line}")]
    Continuation { path: PathBuf, line: usize },
    #[error("failed to read script {path:?}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot statically interpret start script {path:?}:{line}")]
    Statement {
        path: PathBuf,
        line: usize,
        #[source]
        source: anyhow::Error,
    },
    #[error("recursive or excessively deep script reference at {path:?}")]
    Cycle { path: PathBuf },
}
