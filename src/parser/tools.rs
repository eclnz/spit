//! `tool`, `uses` and `anchored` lines: the tools a pipeline names, which of
//! them each operation's jobs depend on, as in `uses register: fsl, !mrtrix3`,
//! and the operations whose tools store their own paths.

use crate::model::{ToolList, UsesDef};

use super::declarations::single_colon;
use super::lexical::{identifier, qualified_identifier};
use super::ParseError;

const EXPECTED_USES: &str =
    "expected `uses operation: tools`, as in `uses register: fsl, !mrtrix3`";

/// The names a `tool` line declares, `line` being the text after `tool`:
/// one or more, separated by commas.
pub(super) fn parse_tool_names(line: &str, number: usize) -> Result<Vec<String>, ParseError> {
    let mut names: Vec<String> = Vec::new();
    for item in line.split(',').map(str::trim) {
        if item.is_empty() {
            return Err(ParseError::new(
                number,
                "expected `tool name`, as in `tool fsl`, or names separated by commas",
            ));
        }
        let name = identifier(item, number, "tool name")?;
        if names.iter().any(|earlier| earlier == name) {
            return Err(
                ParseError::new(number, format!("tool `{name}` is named twice")).at_token(item),
            );
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

/// The operations an `anchored` line names, `line` being the text after
/// `anchored`: one or more, separated by commas.
pub(super) fn parse_anchored(line: &str, number: usize) -> Result<Vec<String>, ParseError> {
    let mut names: Vec<String> = Vec::new();
    for item in line.split(',').map(str::trim) {
        if item.is_empty() {
            return Err(ParseError::new(
                number,
                "expected `anchored operation`, as in `anchored feat`, or operations separated by commas",
            ));
        }
        let name = qualified_identifier(item, number, "anchored operation")?;
        if names.iter().any(|earlier| earlier == name) {
            return Err(
                ParseError::new(number, format!("operation `{name}` is named twice"))
                    .at_token(item),
            );
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

/// A `uses` line, `line` being the text after `uses`.
pub(super) fn parse_uses(line: &str, number: usize) -> Result<UsesDef, ParseError> {
    let delimiter = single_colon(line).ok_or_else(|| ParseError::new(number, EXPECTED_USES))?;
    let (operation, rest) = line.split_at(delimiter);
    let operation = qualified_identifier(operation.trim(), number, "uses operation")?;
    let tools = parse_tool_list(&rest[1..], number)?;
    Ok(UsesDef {
        operation: operation.to_owned(),
        tools,
    })
}

/// A list of tools, as `uses` and `default uses:` give it: names, each
/// with `!` before it to remove it, separated by commas.
pub(super) fn parse_tool_list(list: &str, number: usize) -> Result<ToolList, ParseError> {
    let list = list.trim();
    if list.is_empty() {
        return Err(ParseError::new(
            number,
            "a `uses` list names one or more tools, as in `fsl, !mrtrix3`",
        ));
    }
    let mut tools = ToolList::default();
    for item in list.split(',').map(str::trim) {
        let (removed, written) = match item.strip_prefix('!') {
            Some(name) => (true, name.trim_start()),
            None => (false, item),
        };
        if written.is_empty() {
            return Err(ParseError::new(
                number,
                "a comma in a `uses` list separates two tools, as in `fsl, !mrtrix3`",
            ));
        }
        let name = identifier(written, number, "tool name")?.to_owned();
        if tools.add.contains(&name) || tools.remove.contains(&name) {
            return Err(ParseError::new(
                number,
                format!("tool `{name}` is named twice in one `uses` list"),
            )
            .at_token(item));
        }
        if removed {
            tools.remove.push(name);
        } else {
            tools.add.push(name);
        }
    }
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::{parse_tool_list, parse_tool_names, parse_uses};

    #[test]
    fn a_uses_line_names_an_operation_and_its_tools() {
        let uses = parse_uses("register: fsl, !mrtrix3", 1).expect("a uses line");
        assert_eq!(uses.operation, "register");
        assert_eq!(uses.tools.add, ["fsl"]);
        assert_eq!(uses.tools.remove, ["mrtrix3"]);
    }

    #[test]
    fn bad_lists_are_errors() {
        let error = |list: &str| {
            parse_tool_list(list, 1)
                .expect_err("bad")
                .message()
                .to_owned()
        };
        assert!(error("").contains("one or more tools"));
        assert!(error("fsl,,ants").contains("separates two tools"));
        assert!(error("fsl, !fsl").contains("named twice"));
        assert!(parse_uses("register fsl", 1).is_err());
        assert!(parse_tool_names("fsl, fsl", 1).is_err());
        assert_eq!(
            parse_tool_names("fsl, ants", 1).expect("two names"),
            ["fsl", "ants"]
        );
    }
}
