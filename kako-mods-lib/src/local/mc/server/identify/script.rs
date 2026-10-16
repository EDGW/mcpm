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

#[derive(Clone)]
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
#[derive(Default)]
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
        _ => bail!("unsupported script extension: {path:?}; expected .sh, .bat, or .cmd"),
    };
    let input =
        fs::read_to_string(&path).with_context(|| format!("failed to read script {path:?}"))?;
    let mut joined = String::new();
    let mut first_line = 1;
    let mut terminated = false;
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
        let stop = match statement(&joined, &path, first_line, syntax, state, stack, evidence) {
            Ok(stop) => stop,
            Err(source) => {
                let stop = invalidate(&joined, syntax, state);
                let error = ScriptError::Statement {
                    path: path.to_owned(),
                    line: first_line,
                    source,
                };
                tracing::debug!(error = %format_args!("{error:#}"), "skipping unavailable script evidence");
                evidence.diagnostics.push(error.into());
                stop
            }
        };
        joined.clear();
        if stop {
            terminated = true;
            break;
        }
    }
    if !joined.is_empty() {
        evidence.diagnostics.push(anyhow::anyhow!(
            "unfinished script continuation in {path:?}"
        ));
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
    if !tokens::has_operators(line, syntax)
        && matches!(
            (syntax, head),
            (Syntax::Shell, "echo" | "printf") | (Syntax::Batch, "echo" | "pause")
        )
    {
        return Ok(false);
    }
    if matches!(
        head,
        "if" | "for"
            | "while"
            | "until"
            | "case"
            | "select"
            | "goto"
            | "else"
            | "fi"
            | "done"
            | "do"
            | "("
    ) || head.ends_with("()")
    {
        bail!("control flow are unsupported for evidence extraction");
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
    // Invocation arguments are empty: init does not accept arguments for the script.
    let line = if syntax == Syntax::Shell {
        line.strip_suffix(" \"$@\"").unwrap_or(line)
    } else {
        line.strip_suffix(" %*").unwrap_or(line)
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
    let raw_head = line.split_whitespace().next().unwrap_or("");
    let binary = raw_head.rsplit(['/', '\\']).next().unwrap_or(raw_head);
    let extension = Path::new(raw_head)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !tokens::has_operators(line, syntax)
        && raw_head
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
        )?);
        return Ok(exec);
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

/// Discard only affected state. Unknown control flow ends this script scan because
/// subsequent lines may be conditional; commands already extracted remain available.
fn invalidate(line: &str, syntax: Syntax, state: &mut State) -> bool {
    let line = line.trim().trim_start_matches('@');
    let lower = line.to_ascii_lowercase();
    let head = lower.split_whitespace().next().unwrap_or("");
    if tokens::has_operators(line, syntax) {
        state.vars.clear();
        state.cwd_known = false;
        state.directories.clear();
        state.scopes.clear();
        return true;
    }
    if matches!(head, "cd" | "chdir" | "pushd" | "popd") {
        state.cwd_known = false;
        state.directories.clear();
        return false;
    }
    let assignment = if syntax == Syntax::Batch {
        line.strip_prefix("set ")
            .or_else(|| line.strip_prefix("SET "))
    } else {
        Some(line.strip_prefix("export ").unwrap_or(line))
    };
    if let Some((name, _)) = assignment.and_then(|value| value.trim_matches('"').split_once('=')) {
        if valid_name(name) {
            let name = if syntax == Syntax::Batch {
                name.to_ascii_uppercase()
            } else {
                name.to_owned()
            };
            state.vars.remove(&name);
            return false;
        }
    }
    if matches!(
        head,
        "setlocal" | "endlocal" | "unset" | "export" | "set" | "read" | "shift"
    ) {
        state.vars.clear();
        state.scopes.clear();
        return false;
    }
    let binary = head.rsplit(['/', '\\']).next().unwrap_or(head);
    if !tokens::has_operators(line, syntax)
        && (matches!(binary, "java" | "java.exe") || head == "exec")
    {
        return false;
    }
    // Unknown syntax or a script reference may modify both variables and cwd.
    state.vars.clear();
    state.cwd_known = false;
    state.directories.clear();
    state.scopes.clear();
    true
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
