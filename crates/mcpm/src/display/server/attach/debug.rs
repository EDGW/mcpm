//! Frontend-only boolean debug variables controlled via `@debug <var_name> <true|false>`.

/// Metadata describing one frontend attach debug variable.
pub(super) struct Descriptor {
    pub name: &'static str,
    pub description: &'static str,
}

/// Authoritative list of frontend attach debug variables.
pub(super) const VARIABLES: &[Descriptor] = &[Descriptor {
    name: "display_content_range",
    description: "underline the header-stripped content range on recognized log lines",
}];

/// Session-local boolean debug flags owned by the attach frontend display.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct DebugVariables {
    pub display_content_range: bool,
}

impl DebugVariables {
    pub fn get(&self, name: &str) -> Option<bool> {
        match name {
            "display_content_range" => Some(self.display_content_range),
            _ => None,
        }
    }

    pub fn set(&mut self, name: &str, value: bool) -> bool {
        match name {
            "display_content_range" => {
                self.display_content_range = value;
                true
            }
            _ => false,
        }
    }

    /// Executes `@debug` arguments and returns the human-readable notice to display.
    pub fn apply(&mut self, args: &str) -> String {
        let mut parts = args.split_whitespace();
        let Some(name) = parts.next() else {
            return self.format_all();
        };
        let Some(raw_value) = parts.next() else {
            return match self.get(name) {
                Some(current) => format!("{name} = {current}"),
                None => unknown_variable_message(name),
            };
        };
        if parts.next().is_some() {
            return "usage: @debug <var_name> <true|false>".to_owned();
        }
        let value = match raw_value {
            "true" => true,
            "false" => false,
            _ => {
                return format!(
                    "invalid boolean {raw_value:?}; expected true or false (usage: @debug <var_name> <true|false>)"
                );
            }
        };
        if self.set(name, value) {
            format!("{name} = {value}")
        } else {
            unknown_variable_message(name)
        }
    }

    fn format_all(&self) -> String {
        VARIABLES
            .iter()
            .map(|entry| {
                let value = self.get(entry.name).unwrap_or(false);
                format!("{} = {value} ({})", entry.name, entry.description)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn unknown_variable_message(name: &str) -> String {
    let available = VARIABLES
        .iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>()
        .join(", ");
    format!("unknown debug variable {name:?}; available: {available}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_get_set_list_and_validation_for_debug_variables() {
        let mut vars = DebugVariables::default();
        assert!(!vars.display_content_range);
        assert!(vars.apply("").contains("display_content_range = false"));
        assert_eq!(vars.apply("display_content_range"), "display_content_range = false");

        assert_eq!(
            vars.apply("display_content_range true"),
            "display_content_range = true"
        );
        assert!(vars.display_content_range);

        assert_eq!(
            vars.apply("display_content_range false"),
            "display_content_range = false"
        );
        assert!(!vars.display_content_range);

        assert!(vars.apply("display_content_range yes").contains("expected true or false"));
        assert!(vars.apply("unknown_var true").contains("unknown debug variable"));
        assert!(vars.apply("display_content_range true extra").contains("usage:"));
    }
}
