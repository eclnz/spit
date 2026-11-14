//! `.spitenv` files: what each tool is on the machine the jobs run on, which
//! a recipe names with `tools from file.spitenv`.

use std::path::Path;

use crate::model::{Pipeline, Tool, ToolIdentity};
use crate::parser::{without_bom, ParseError};
use crate::span::strip_comment;
use crate::template::{CommandPart, CommandTemplate};

const HEADER: &str = "a .spitenv is one `tools:` block, with a `name: identity` line indented beneath it for each tool, as in `fsl: probe flirt -version`";

/// The tools a recipe's `tools from` line names: the file as written, the
/// line, and what the file says each tool is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolsFile {
    /// The file, relative to the recipe's folder.
    pub file: String,
    /// The recipe line that names it.
    pub line: usize,
    /// Each tool, in the order the file lists them.
    pub tools: Vec<Tool>,
}

/// The tools a `.spitenv`'s text lists, in order. A line's errors are on its
/// own line.
pub fn parse_tools(text: &str) -> Result<Vec<Tool>, ParseError> {
    let mut tools: Vec<Tool> = Vec::new();
    let mut header = false;
    for (index, original) in without_bom(text).lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(original).trim();
        if line.is_empty() {
            continue;
        }
        let indented = original.starts_with([' ', '\t']);
        if !header {
            if line != "tools:" || indented {
                return Err(ParseError::new(number, HEADER));
            }
            header = true;
            continue;
        }
        if !indented {
            return Err(ParseError::new(number, HEADER));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| ParseError::new(number, HEADER))?;
        let name = name.trim();
        let valid = name.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(ParseError::new(
                number,
                format!("`{name}` is not a tool name: use letters, digits and `_`, starting with a letter"),
            ));
        }
        if tools.iter().any(|tool| tool.name == name) {
            return Err(ParseError::new(
                number,
                format!("tool `{name}` is listed twice"),
            ));
        }
        let identity = identity(value.trim(), name, number)?;
        tools.push(Tool {
            name: name.to_owned(),
            identity,
        });
    }
    if !header || tools.is_empty() {
        return Err(ParseError::new(1, HEADER));
    }
    Ok(tools)
}

/// What one tool's line says it is: `probe` and a command, or a text.
fn identity(value: &str, name: &str, number: usize) -> Result<ToolIdentity, ParseError> {
    if value.is_empty() {
        return Err(ParseError::new(
            number,
            format!("say what `{name}` is: a text such as a container digest, or `probe` and a command whose output identifies it"),
        ));
    }
    let Some(command) = value
        .strip_prefix("probe")
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    else {
        return Ok(ToolIdentity::Declared(value.to_owned()));
    };
    let template = CommandTemplate::parse(command.trim()).map_err(|error| {
        ParseError::new(number, format!("`{name}`'s probe: {}", error.message()))
    })?;
    if !template.redirects().is_empty() || !template.shell_operators().is_empty() {
        return Err(ParseError::new(
            number,
            format!("`{name}`'s probe runs with no shell, so `<`, `>` and `|` are not its; run `sh -c '...'` for a pipeline of commands"),
        ));
    }
    template
        .arguments()
        .iter()
        .map(|argument| {
            argument
                .iter()
                .map(|part| match part {
                    CommandPart::Literal(text) => Ok(text.as_str()),
                    _ => Err(ParseError::new(
                        number,
                        format!("`{name}`'s probe is run as written, so it takes no `{{...}}` or `@scalar`; quote a brace to pass it"),
                    )),
                })
                .collect::<Result<String, _>>()
        })
        .collect::<Result<Vec<_>, _>>()
        .map(ToolIdentity::Probe)
}

/// Read and parse the `.spitenv` at `folder` joined with `file`, which the
/// recipe names on `line`. Its errors are given on that line, naming the
/// file and its own line.
pub(crate) fn read_tools_file(
    folder: &Path,
    file: &str,
    line: usize,
) -> Result<ToolsFile, ParseError> {
    let text = std::fs::read_to_string(folder.join(file)).map_err(|reason| {
        ParseError::new(
            line,
            format!("cannot read `{file}` for `tools from`: {reason}"),
        )
    })?;
    let tools = parse_tools(&text).map_err(|error| {
        ParseError::new(
            line,
            format!("{file} line {}: {}", error.line(), error.message()),
        )
    })?;
    Ok(ToolsFile {
        file: file.to_owned(),
        line,
        tools,
    })
}

/// The first tool `pipeline` declares that `tools` does not say what it
/// is, if any.
pub(crate) fn missing_tool<'p>(pipeline: &'p Pipeline, tools: &ToolsFile) -> Option<&'p str> {
    pipeline
        .tools
        .iter()
        .find(|name| !tools.tools.iter().any(|tool| &tool.name == *name))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::parse_tools;
    use crate::model::ToolIdentity;

    #[test]
    fn a_tool_is_a_text_or_a_probe() {
        let tools = parse_tools(
            "# this machine\ntools:\n    mrtrix3: probe mrconvert -version\n    fsl: probe sh -c 'cat \"$FSLDIR/etc/fslversion\"'\n    synthseg: docker.io/lab/synthseg@sha256:4f1c\n",
        )
        .expect("a valid .spitenv");
        assert_eq!(
            tools[0].identity,
            ToolIdentity::Probe(vec!["mrconvert".into(), "-version".into()])
        );
        assert_eq!(
            tools[1].identity,
            ToolIdentity::Probe(vec![
                "sh".into(),
                "-c".into(),
                "cat \"$FSLDIR/etc/fslversion\"".into()
            ])
        );
        assert_eq!(
            tools[2].identity,
            ToolIdentity::Declared("docker.io/lab/synthseg@sha256:4f1c".into())
        );
    }

    #[test]
    fn mistakes_are_on_their_lines() {
        let error = |text: &str| parse_tools(text).expect_err("invalid");
        assert_eq!(error("tools:\n  a: probe\n").line(), 2);
        assert_eq!(error("tools:\n  a: x\n  a: y\n").line(), 3);
        assert_eq!(error("tools:\n  a: x\nb: y\n").line(), 3);
        assert_eq!(error("tools:\n  a: probe ls > out\n").line(), 2);
        assert_eq!(error("tools:\n  a: probe ls {x}\n").line(), 2);
        assert_eq!(error("fsl: x\n").line(), 1);
        assert!(parse_tools("tools:\n").is_err());
    }
}
