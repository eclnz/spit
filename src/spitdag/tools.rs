//! Tool identities are supplied by the environment, independently of pipeline commands.

use std::collections::BTreeSet;

use crate::span::{content_columns, Place};
use crate::template::{CommandError, CommandPart, CommandTemplate};

/// An opaque identity or a command whose exact stdout identifies an installed tool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tool {
    pub name: String,
    pub identity: Option<String>,
    pub probe: Option<Vec<String>>,
}

/// Parse a `.spitenv` with a `tools:` header and `name: identity` or
/// `name: probe program arguments` entries. Probes contain no placeholders,
/// redirects or implicit shell syntax.
pub fn parse_tools(text: &str) -> Result<Vec<Tool>, CommandError> {
    let mut tools = Vec::new();
    let mut seen = BTreeSet::new();
    let mut header = false;
    for (line, original) in text.lines().enumerate() {
        let text = original.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let fail = |message: String| {
            CommandError::new(message).at(Some(Place::new(line + 1, content_columns(original))))
        };
        if !header {
            if text != "tools:" {
                return Err(fail("a .spitenv starts with `tools:`".to_owned()));
            }
            header = true;
            continue;
        }
        let (name, value) = text
            .split_once(':')
            .ok_or_else(|| fail("expected tool: identity or tool: probe command".to_owned()))?;
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            || !seen.insert(name.to_owned())
        {
            return Err(fail(format!("invalid or duplicate tool name `{name}`")));
        }
        if value.is_empty() {
            return Err(fail(format!("tool `{name}` has no identity")));
        }
        let (identity, probe) = if let Some(command) = value.strip_prefix("probe ") {
            let parsed =
                CommandTemplate::parse(command).map_err(|error| fail(error.to_string()))?;
            if !parsed.redirects().is_empty() || !parsed.shell_operators().is_empty() {
                return Err(fail(
                    "a probe runs one program without redirects or an implicit shell".to_owned(),
                ));
            }
            let arguments = parsed
                .arguments()
                .iter()
                .map(|argument| {
                    let mut text = String::new();
                    for part in argument {
                        let CommandPart::Literal(value) = part else {
                            return Err(fail(
                                "a probe has no placeholders or input reads".to_owned(),
                            ));
                        };
                        text.push_str(value);
                    }
                    if text.contains('\0') {
                        return Err(fail("a probe argument cannot contain NUL".to_owned()));
                    }
                    Ok(text)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if arguments.first().is_none_or(String::is_empty) {
                return Err(fail("a probe needs a program".to_owned()));
            }
            (None, Some(arguments))
        } else {
            (Some(value.to_owned()), None)
        };
        tools.push(Tool {
            name: name.to_owned(),
            identity,
            probe,
        });
    }
    if !header {
        return Err(CommandError::new("a .spitenv starts with `tools:`"));
    }
    Ok(tools)
}

impl super::BoundDag {
    /// Attach an environment and settle each step's tool dependencies once.
    /// A dependency on a tool absent from the environment is an error.
    pub fn set_tools(&mut self, tools: Vec<Tool>) -> Result<(), CommandError> {
        for step in &mut self.steps {
            let mut selected: BTreeSet<String> = match &step.tools {
                None => tools.iter().map(|tool| tool.name.clone()).collect(),
                Some(names) if names.first().is_some_and(|name| name.starts_with('!')) => {
                    tools.iter().map(|tool| tool.name.clone()).collect()
                }
                Some(_) => BTreeSet::new(),
            };
            if let Some(names) = &step.tools {
                for name in names {
                    let bare = name.trim_start_matches('!');
                    if !tools.iter().any(|tool| tool.name == bare) {
                        return Err(CommandError::new(format!(
                            "tool environment does not define `{bare}`"
                        )));
                    }
                    if name.starts_with('!') {
                        selected.remove(bare);
                    } else {
                        selected.insert(name.clone());
                    }
                }
            }
            step.tools = Some(selected.into_iter().collect());
        }
        self.tools = tools;
        Ok(())
    }
}
