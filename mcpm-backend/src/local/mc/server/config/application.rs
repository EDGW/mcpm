//! Recognized Minecraft option boundaries shared by import and duplicate validation.

use anyhow::Result;
use std::ops::Range;

/// Returns tunable options only when every application operand boundary is known.
/// Unknown grammars remain an intact fixed argv region rather than being guessed.
pub(super) fn settings(arguments: &[String]) -> Result<Option<Vec<Setting<'_>>>> {
    let mut result = Vec::new();
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        let start = index;
        let (name, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(key, value)| (key, Some(value)));
        let field = match name {
            "nogui" | "--nogui" if inline.is_none() => Some("server.nogui"),
            "--port" => Some("server.port"),
            "--world" => Some("server.world"),
            _ => None,
        };
        let operand = matches!(
            name,
            "--port"
                | "--world"
                | "--universe"
                | "--config"
                | "--owner"
                | "--singleplayer"
                | "--serverId"
                | "--server-id"
                | "--pidFile"
                | "--launchTarget"
        ) || name.starts_with("--fml.");
        let flag = matches!(
            name,
            "nogui"
                | "--nogui"
                | "--demo"
                | "--bonusChest"
                | "--forceUpgrade"
                | "--eraseCache"
                | "--recreateRegionFiles"
                | "--safeMode"
                | "--initSettings"
                | "--help"
                | "--jfrProfile"
        );
        if !(operand || flag && inline.is_none()) {
            return Ok(None);
        }
        let value = if operand {
            Some(if let Some(value) = inline {
                value
            } else {
                index += 1;
                let Some(value) = arguments.get(index) else {
                    return Ok(None);
                };
                value.as_str()
            })
        } else {
            None
        };
        index += 1;
        if let Some(field) = field {
            result.push(Setting {
                range: start..index,
                field,
                value,
            });
        }
    }
    Ok(Some(result))
}

/// A tunable option and its exact source span, excluding operands of unrelated options.
pub(super) struct Setting<'a> {
    pub range: Range<usize>,
    pub field: &'static str,
    pub value: Option<&'a str>,
}
