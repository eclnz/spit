//! The flow form: `source` and `operation` declarations, steps written
//! `outputs = operation(inputs)`, and stages whose lines are indented
//! beneath a `stage name:` header.

use crate::model::{CommandRole, Invocation};

use super::declarations::{parse_discover, parse_invocation_parts, parse_path, parse_product};
use super::keyword::Keyword;
use super::lexical::{comma_items, identifier, strip_comment};
use super::source_map::{name_place, step_place};
use super::{FlowOutput, FlowStep, ParseError, StatementKind, Syntax, SHELL_SOURCE_REMOVED};

pub(super) fn parse_flow(text: &str) -> Syntax {
    let mut syntax = Syntax::default();
    let mut stages = OpenStages::default();
    for (index, original) in text.lines().enumerate() {
        if let Err(error) = flow_line(&mut syntax, &mut stages, original, index + 1) {
            syntax.error = Some(error.locate(original));
            break;
        }
    }
    syntax
}

/// The stages open at a line of the flow form, outermost first.
#[derive(Default)]
struct OpenStages(Vec<OpenStage>);

struct OpenStage {
    /// The full name, such as `preprocess/denoise`.
    name: String,
    /// The indentation of the stage's header.
    header: usize,
    /// The indentation the stage's lines share, once the first is read.
    body: Option<usize>,
}

impl OpenStages {
    /// The innermost open stage.
    fn current(&self) -> Option<&str> {
        self.0.last().map(|stage| stage.name.as_str())
    }

    /// Close each stage that a line indented by `indent` is not inside, then
    /// check that the line lines up with the other lines of its stage.
    fn enter(&mut self, indent: usize, number: usize) -> Result<(), ParseError> {
        while self.0.last().is_some_and(|stage| indent <= stage.header) {
            self.0.pop();
        }
        let Some(stage) = self.0.last_mut() else {
            return Ok(());
        };
        match stage.body {
            None => stage.body = Some(indent),
            Some(body) if body == indent => {}
            Some(_) => {
                return Err(ParseError::new(
                    number,
                    format!(
                        "this line is indented differently from the other lines of stage `{}`",
                        stage.name
                    ),
                ))
            }
        }
        Ok(())
    }
}

/// Open a stage for a `stage name:` header, inside the current stage if
/// there is one. Its lines are indented beneath it, and the next line that
/// is not ends it.
fn open_stage(
    syntax: &mut Syntax,
    stages: &mut OpenStages,
    original: &str,
    declaration: &str,
    indent: usize,
    number: usize,
) -> Result<(), ParseError> {
    let syntax_error = "expected `stage name:`, with the stage's lines indented beneath it";
    if stages.current().is_none() && indent > 0 {
        return Err(ParseError::new(
            number,
            "a stage header outside every stage starts at the beginning of its line",
        ));
    }
    let name = declaration
        .trim()
        .strip_suffix(':')
        .ok_or_else(|| ParseError::new(number, syntax_error))?;
    let name = identifier(name.trim(), number, "stage name")?;
    let full = match stages.current() {
        Some(parent) => format!("{parent}/{name}"),
        None => name.to_owned(),
    };
    let place = name_place(original, number, declaration, name);
    syntax.push(
        original,
        number,
        StatementKind::Stage {
            name: full.clone(),
            place,
        },
    );
    stages.0.push(OpenStage {
        name: full,
        header: indent,
        body: None,
    });
    Ok(())
}

fn flow_line(
    syntax: &mut Syntax,
    stages: &mut OpenStages,
    original: &str,
    number: usize,
) -> Result<(), ParseError> {
    let line = strip_comment(original).trim();
    if line.is_empty() {
        return Ok(());
    }
    let indent = original.len() - original.trim_start().len();
    stages.enter(indent, number)?;
    let stage = stages.current().map(str::to_owned);
    let top_level_only = |what: &str| {
        stage.as_ref().map_or(Ok(()), |name| {
            Err(ParseError::new(
                number,
                format!("{what} belongs at the top level, outside stage `{name}`"),
            ))
        })
    };
    let kind = match Keyword::split(line) {
        Some((Keyword::Stage, declaration)) => {
            return open_stage(syntax, stages, original, declaration, indent, number);
        }
        Some((Keyword::Use, _)) => {
            top_level_only("`use`")?;
            StatementKind::Import
        }
        Some((Keyword::Source, declaration)) => {
            top_level_only("`source`, which declares an input,")?;
            StatementKind::product(original, declaration.trim(), number)?
        }
        Some((Keyword::Discover, declaration)) => {
            top_level_only("`discover`")?;
            StatementKind::Discover(parse_discover(declaration.trim(), number)?)
        }
        Some((Keyword::Operation, declaration)) => {
            StatementKind::operation(original, declaration.trim(), number)?
        }
        Some((keyword @ (Keyword::Require | Keyword::Skip | Keyword::Drop), _)) => {
            top_level_only(if keyword == Keyword::Require {
                "`require`, which checks sources,"
            } else {
                "`drop`, which removes groups,"
            })?;
            StatementKind::constraint(original, line, number)?
        }
        Some((Keyword::Exclude, declaration)) => {
            top_level_only("`exclude`, which removes sources,")?;
            StatementKind::exclude(original, declaration, number)?
        }
        Some((keyword @ (Keyword::Command | Keyword::Verify), declaration)) => {
            let role = if keyword == Keyword::Command {
                CommandRole::Run
            } else {
                CommandRole::Verify
            };
            StatementKind::command(original, declaration.trim(), number, role)?
        }
        Some((Keyword::ShellSource, _)) => {
            return Err(ParseError::new(number, SHELL_SOURCE_REMOVED));
        }
        Some((Keyword::Path, _)) => StatementKind::Path(parse_path(stage, original, line, number)?),
        None => flow_statement(original, line, number, stage.as_deref())?,
    };
    syntax.push(original, number, kind);
    Ok(())
}

/// A line that starts with no keyword: a step, or a mistake.
fn flow_statement(
    original: &str,
    line: &str,
    number: usize,
    stage: Option<&str>,
) -> Result<StatementKind, ParseError> {
    if let Some(word) = unknown_keyword(line) {
        let hint = STATEMENT_WORDS
            .iter()
            .find(|keyword| edit_distance(word, keyword) <= 2)
            .map_or_else(String::new, |keyword| format!("did you mean `{keyword}`? "));
        return Err(ParseError::new(
            number,
            format!(
                "`{word}` does not start a statement; {hint}a pipeline line starts with \
                 source, operation, command, verify, path, stage or use, or is a step \
                 `output = operation(inputs)`, and a recipe line starts with pipeline, \
                 discover, require, drop, exclude or path"
            ),
        )
        .at_token(word));
    }
    if line.contains('=') {
        let (mut invocation, outputs) = parse_flow_step(line, number)?;
        invocation.stage = stage.map(str::to_owned);
        let step = step_place(original, number, &invocation);
        Ok(StatementKind::FlowStep(FlowStep {
            invocation,
            outputs,
            step,
        }))
    } else if line.contains('(') && line.ends_with(')') {
        Err(ParseError::new(
            number,
            "expected `=` before operation call",
        ))
    } else {
        Err(ParseError::new(
            number,
            "expected source, discover, operation, command, verify, require, path, stage, or output = operation(inputs)",
        ))
    }
}

/// Parse `outputs = operation(inputs)`, where each output may declare its
/// product's type and dimensions.
fn parse_flow_step(line: &str, number: usize) -> Result<(Invocation, Vec<FlowOutput>), ParseError> {
    let (left, call) = line
        .split_once('=')
        .ok_or_else(|| ParseError::new(number, "expected flow step: output = operation(inputs)"))?;
    let outputs = comma_items(left, number)?
        .into_iter()
        .map(|output| parse_flow_output(output, number))
        .collect::<Result<Vec<_>, _>>()?;
    if outputs.is_empty() {
        return Err(ParseError::new(
            number,
            "expected an output product before `=`",
        ));
    }
    let names = outputs.iter().map(|output| output.name.clone()).collect();
    let invocation = parse_invocation_parts(names, call, number)?;
    Ok((invocation, outputs))
}

fn parse_flow_output(left: &str, number: usize) -> Result<FlowOutput, ParseError> {
    if left.contains(':') {
        let product = parse_product(left, number)?;
        Ok(FlowOutput {
            name: product.name,
            artifact_type: Some(product.artifact_type),
            dimensions: Some(product.dimensions),
        })
    } else {
        Ok(FlowOutput {
            name: identifier(left, number, "output product")?.to_owned(),
            artifact_type: None,
            dimensions: None,
        })
    }
}

/// The words a statement can start with, for suggesting one.
const STATEMENT_WORDS: [&str; 12] = [
    "source",
    "operation",
    "command",
    "verify",
    "path",
    "stage",
    "use",
    "pipeline",
    "discover",
    "require",
    "drop",
    "exclude",
];

/// The first word of `line`, when it reads as an unknown keyword: a
/// lowercase word followed by more than a step's `=`, `,` or `: Type` can
/// follow it with, as in `omit bold[sub=02]`.
fn unknown_keyword(line: &str) -> Option<&str> {
    // A line shaped as a step, or as a call missing its `=`, is reported as
    // one, whatever is wrong in it.
    let step = line
        .find('(')
        .is_some_and(|paren| line[..paren].contains('='));
    if step || (line.contains('(') && line.ends_with(')')) {
        return None;
    }
    let (word, rest) = line.split_once(char::is_whitespace)?;
    let is_word = word.starts_with(|first: char| first.is_ascii_lowercase())
        && word
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    (is_word && !rest.trim_start().starts_with(['=', ',', ':'])).then_some(word)
}

/// How many single-character insertions, deletions and substitutions turn
/// `left` into `right`.
fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut row: Vec<usize> = (0..=right.len()).collect();
    for (index, left_char) in left.chars().enumerate() {
        let mut previous = row[0];
        row[0] = index + 1;
        for (column, &right_char) in right.iter().enumerate() {
            let substitution = previous + usize::from(left_char != right_char);
            previous = row[column + 1];
            row[column + 1] = substitution.min(row[column] + 1).min(previous + 1);
        }
    }
    row[right.len()]
}

#[cfg(test)]
mod unknown_tests {
    use super::{edit_distance, unknown_keyword};

    #[test]
    fn a_line_that_cannot_start_a_step_names_its_first_word() {
        assert_eq!(unknown_keyword("omit bold[sub=02]"), Some("omit"));
        assert_eq!(unknown_keyword("drop [sub] where x count<2"), Some("drop"));
        assert_eq!(unknown_keyword("cleaned = clean(raw)"), None);
        assert_eq!(unknown_keyword("low, high = split(x)"), None);
        assert_eq!(
            unknown_keyword("mean : Image [s] = average(x @ vary(r))"),
            None
        );
        assert_eq!(unknown_keyword("Cleaned stuff"), None);
        assert_eq!(unknown_keyword("bad name = copy(raw)"), None);
        assert_eq!(unknown_keyword("result copy(raw)"), None);
        assert_eq!(unknown_keyword("operaton clean(x) -> Y"), Some("operaton"));
        assert_eq!(edit_distance("excldue", "exclude"), 2);
        assert_eq!(edit_distance("", "use"), 3);
    }
}
