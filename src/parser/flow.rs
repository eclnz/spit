//! The flow form: `source` and `operation` declarations, steps written
//! `outputs = operation(inputs)`, and stages whose lines are indented
//! beneath a `stage name:` header.

use crate::model::{CommandRole, Invocation, SidecarGroup};
use crate::paths::PathTemplate;

use super::declarations::{
    parse_dimension_order, parse_discover, parse_invocation_parts, parse_path, parse_product,
};
use super::keyword::{removed_section, Keyword};
use super::lexical::{comma_items, extension, identifier, strip_comment};
use super::source_map::{name_place, step_place, tail_place};
use super::{
    FlowOutput, FlowStep, ParseError, PathRule, StatementKind, Syntax, SHELL_SOURCE_REMOVED,
};

pub(super) fn parse_flow(text: &str) -> Syntax {
    let mut syntax = Syntax::default();
    let mut stages = OpenStages::default();
    let mut group = None;
    for (index, original) in text.lines().enumerate() {
        if let Err(error) = flow_line(&mut syntax, &mut stages, &mut group, original, index + 1) {
            syntax.error = Some(error.locate(original));
            return syntax;
        }
    }
    if let Some(group) = group {
        let header = group.header.clone();
        if let Err(error) = group.close(&mut syntax) {
            syntax.error = Some(error.locate(&header));
        }
    }
    syntax
}

/// An open `sidecars` block: its header, and the members read so far.
struct OpenGroup {
    group: SidecarGroup,
    /// The header line, and the path stem every member's path starts with.
    header: String,
    number: usize,
    stem: String,
}

impl OpenGroup {
    /// Open a block for `sidecars name [dimensions]: stem`.
    fn open(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let expected = "expected `sidecars name [dimensions]: path stem`, with each member indented beneath it as `source name : Type .ext`";
        let (head, stem) = declaration
            .split_once(':')
            .ok_or_else(|| ParseError::new(number, expected))?;
        let (name, dimensions) = match head.split_once('[') {
            Some((name, dimensions)) => {
                let dimensions = dimensions.trim().strip_suffix(']').ok_or_else(|| {
                    ParseError::new(number, "expected closing `]` after the group's dimensions")
                })?;
                let dimensions = comma_items(dimensions, number)?
                    .into_iter()
                    .map(|dimension| identifier(dimension, number, "dimension").map(str::to_owned))
                    .collect::<Result<Vec<_>, _>>()?;
                (name, dimensions)
            }
            None => (head, Vec::new()),
        };
        let name = identifier(name.trim(), number, "sidecars group name")?;
        let stem = stem.trim();
        if stem.is_empty() {
            return Err(ParseError::new(number, expected));
        }
        PathTemplate::parse(stem)
            .map_err(|error| ParseError::new(number, error.message()).at_token(stem))?;
        Ok(Self {
            group: SidecarGroup {
                name: name.to_owned(),
                dimensions,
                members: Vec::new(),
            },
            header: original.to_owned(),
            number,
            stem: stem.to_owned(),
        })
    }

    /// Add a member, `source name : Type .ext`: a source with the group's
    /// dimensions, whose path is the stem and its extension.
    fn member(
        &mut self,
        syntax: &mut Syntax,
        original: &str,
        line: &str,
        number: usize,
    ) -> Result<(), ParseError> {
        let group = &self.group.name;
        let Some((Keyword::Source, declaration)) = Keyword::split(line) else {
            return Err(ParseError::new(
                number,
                format!("sidecars group `{group}` holds only its sources, each written `source name : Type .ext`"),
            ));
        };
        let Some((declaration, written)) = declaration.split_once('.') else {
            return Err(ParseError::new(
                number,
                format!("a source in sidecars group `{group}` names the extension its file adds to the stem, as in `source gps : GpsTrack .gpx`"),
            ));
        };
        if let Some(bracket) = declaration.find('[') {
            return Err(ParseError::new(
                number,
                format!("a source in sidecars group `{group}` takes the group's dimensions; remove its own"),
            )
            .at_token(&declaration[bracket..]));
        }
        let extension = extension(&format!(".{}", written.trim()), number)?.to_owned();
        let declaration = declaration.trim();
        let StatementKind::Product(mut product, place) =
            StatementKind::product(original, declaration, number)?
        else {
            unreachable!("a source line declares a product");
        };
        product.dimensions.clone_from(&self.group.dimensions);
        let template = PathTemplate::parse(format!("{}{extension}", self.stem))
            .map_err(|error| ParseError::new(number, error.message()))?;
        let rule = PathRule {
            product: Some(product.name.clone()),
            stage: None,
            template,
            // A member's path is the group's stem.
            place: tail_place(&self.header, self.number, &self.stem),
        };
        self.group.members.push((product.name.clone(), extension));
        syntax.push(original, number, StatementKind::Product(product, place));
        syntax.push(original, number, StatementKind::Path(rule));
        Ok(())
    }

    /// End the block, recording the group once its members are read.
    fn close(self, syntax: &mut Syntax) -> Result<(), ParseError> {
        if self.group.members.is_empty() {
            return Err(ParseError::new(
                self.number,
                format!(
                    "sidecars group `{}` has no sources; indent each beneath its header as `source name : Type .ext`",
                    self.group.name
                ),
            ));
        }
        syntax.push(
            &self.header,
            self.number,
            StatementKind::SidecarGroup(self.group),
        );
        Ok(())
    }
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
    group: &mut Option<OpenGroup>,
    original: &str,
    number: usize,
) -> Result<(), ParseError> {
    let line = strip_comment(original).trim();
    if line.is_empty() {
        return Ok(());
    }
    // An indented line belongs to an open `sidecars` block; the next that
    // is not ends it.
    let indented = original.starts_with(char::is_whitespace);
    if let Some(open) = group.as_mut() {
        if indented {
            return open.member(syntax, original, line, number);
        }
        group.take().expect("a group is open").close(syntax)?;
    }
    if let Some(instead) = removed_section(line) {
        return Err(ParseError::new(
            number,
            format!("SPIT no longer reads `{line}` sections; {instead}"),
        ));
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
        Some((Keyword::Sidecars, declaration)) => {
            top_level_only("`sidecars`, which declares inputs,")?;
            if indent > 0 {
                return Err(ParseError::new(
                    number,
                    "a `sidecars` header starts at the beginning of its line",
                ));
            }
            *group = Some(OpenGroup::open(original, declaration, number)?);
            return Ok(());
        }
        Some((Keyword::Use, _)) => {
            top_level_only("`use`")?;
            StatementKind::Import
        }
        Some((Keyword::Source, declaration)) => {
            top_level_only("`source`, which declares an input,")?;
            StatementKind::product(original, declaration.trim(), number)?
        }
        Some((Keyword::Dimensions, declaration)) => {
            top_level_only("`dimensions`, which orders the whole pipeline,")?;
            StatementKind::Dimensions(parse_dimension_order(declaration, number)?)
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
        Some((Keyword::Ext, rest)) => StatementKind::Extension {
            stage,
            extension: extension(rest.trim(), number)?.to_owned(),
        },
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
                 source, sidecars, dimensions, operation, command, verify, path, ext, stage or use, or is a step \
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
const STATEMENT_WORDS: [&str; 15] = [
    "source",
    "sidecars",
    "dimensions",
    "operation",
    "command",
    "verify",
    "path",
    "ext",
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
