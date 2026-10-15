use super::{
    java::{self, ServerLaunch},
    tokens::{self, Syntax},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

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

#[derive(Clone)]
struct State {
    cwd: PathBuf,
    vars: HashMap<String, String>,
    directories: Vec<PathBuf>,
    scopes: Vec<HashMap<String, String>>,
    exported: HashSet<String>,
    shell_script: PathBuf,
    shell_directory: PathBuf,
}

pub(super) fn read(root: &Path, path: &Path) -> Result<Vec<ServerLaunch>> {
    let mut state = State {
        cwd: root.to_owned(),
        vars: HashMap::new(),
        directories: vec![],
        scopes: vec![],
        exported: HashSet::new(),
        shell_script: path.to_owned(),
        shell_directory: root.to_owned(),
    };
    let mut launches = Vec::new();
    visit(path, &mut state, &mut HashSet::new(), &mut launches)?;
    Ok(launches)
}

fn visit(
    path: &Path,
    state: &mut State,
    stack: &mut HashSet<PathBuf>,
    launches: &mut Vec<ServerLaunch>,
) -> Result<bool> {
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
        let stop = statement(&joined, &path, first_line, syntax, state, stack, launches).map_err(
            |source| ScriptError::Statement {
                path: path.to_owned(),
                line: first_line,
                source,
            },
        )?;
        joined.clear();
        if stop {
            terminated = true;
            break;
        }
    }
    ensure!(
        joined.is_empty(),
        "unfinished script continuation in {path:?}"
    );
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
    launches: &mut Vec<ServerLaunch>,
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
        let values = tokens::split(value, syntax, &state.vars, expansion_script)?;
        ensure!(
            values.len() <= 1,
            "variable assignment needs one literal value"
        );
        state.vars.insert(
            if syntax == Syntax::Batch {
                name.to_ascii_uppercase()
            } else {
                name.into()
            },
            values.first().cloned().unwrap_or_default(),
        );
        if syntax == Syntax::Shell && line.starts_with("export ") {
            state.exported.insert(name.into());
        }
        return Ok(false);
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
        let new = fs::canonicalize(resolve(&state.cwd, args[0], syntax))
            .context("failed to resolve script working directory")?;
        ensure!(new.is_dir(), "script working directory is not a directory");
        if command == "pushd" {
            state.directories.push(state.cwd.clone());
        }
        state.cwd = new;
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
    if command == "exit" {
        bail!("explicit exit/return control flow is unsupported");
    }
    let binary = command.rsplit(['/', '\\']).next().unwrap();
    if matches!(binary, "java" | "java.exe") {
        for name in [
            "JDK_JAVA_OPTIONS",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "CLASSPATH",
        ] {
            ensure!(
                !state.vars.contains_key(name),
                "implicit Java environment option {name} is unsupported"
            );
        }
        launches.push(java::parse(
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
        ensure!(words.len() == 1, "unsupported command {command:?}");
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
        let terminated = visit(&child, state, stack, launches)?;
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
        visit(&child, &mut child_state, stack, launches)?;
    }
    Ok(exec || (syntax == Syntax::Batch && !reference))
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
