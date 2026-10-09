//! The flow form: `source` and `operation` declarations, steps written
//! `outputs = operation(inputs)`, and stages whose lines are indented
//! beneath a `stage name:` header.

use crate::model::{CommandRole, DefaultChecks, Invocation};

use super::body::OpenBody;
use super::check::parse_default_checks;
use super::declarations::{
    parse_dimension_order, parse_discover, parse_invocation_parts, parse_path, parse_product,
};
use super::keyword::{removed_section, Keyword};
use super::lexical::{comma_items, extension, identifier, strip_comment};
use super::source_map::{name_place, step_place, tail_place};
use super::{FlowStep, ParseError, StatementKind, StepOutput, Syntax, SHELL_SOURCE_REMOVED};

/// Parse the lines of `text`, reading on past a line that fails as if it
/// were blank, so that one pass finds the errors that blanking each failed
/// line in turn would, one error per pass. Where a blank line would not
/// leave the rest reading the same, parsing stops after the error instead.
pub(super) fn parse_flow(text: &str) -> Syntax {
    let mut syntax = Syntax::default();
    let mut open = Open::default();
    // After a line that ended a body: the indentation of the body's header,
    // and the statement the line made, if it did not fail. Were that line
    // blank, the body would stay open, and so it would past each line after
    // it that is blank in turn, so a later line indented beneath the header
    // would belong to the body. A line that cannot be blank ends the watch.
    let mut ended_body: Option<(Option<usize>, usize)> = None;
    // The statements that closed stages, with the indentation of the header
    // of the outermost stage each closed, which a later line may still be
    // indented beneath. Were such a statement blank, the stage would stay
    // open, and that line would belong to it. The headers fall as the
    // statements come. A statement that cannot fail ends the watch: it
    // closes the stage whether or not the others are blank.
    let mut closed: Vec<(usize, usize)> = Vec::new();
    for (index, logical) in super::continuation::operation_lines(text) {
        let original = logical.as_ref();
        if !closed.is_empty() && !strip_comment(original).trim().is_empty() {
            let indent = original.len() - original.trim_start().len();
            while let Some(&(statement, header)) = closed.last() {
                if indent <= header {
                    break;
                }
                syntax.statements[statement].stateful = true;
                closed.pop();
            }
        }
        if let Some((statement, header)) = ended_body {
            if !strip_comment(original).trim().is_empty()
                && original.len() - original.trim_start().len() > header
            {
                ended_body = None;
                match statement {
                    Some(statement) => syntax.statements[statement].stateful = true,
                    None => return syntax,
                }
            }
        }
        let number = index + 1;
        let before = syntax.statements.len();
        match flow_line(&mut syntax, &mut open, original, number) {
            Ok(effect) => {
                // The statement the line made, if it made one.
                let made =
                    syntax.statements.len().checked_sub(1).filter(|&last| {
                        last >= before && syntax.statements[last].place.line == number
                    });
                if let Some(made) = made {
                    if let Some(header) = effect.closed {
                        closed.push((made, header));
                    }
                    if let Some(header) = effect.ended_body {
                        ended_body = Some((Some(made), header));
                    } else if matches!(
                        syntax.statements[made].kind,
                        StatementKind::Stage { .. }
                            | StatementKind::Command(..)
                            | StatementKind::Check(..)
                    ) {
                        // Lowering cannot fail these.
                        ended_body = None;
                        closed.clear();
                    }
                }
            }
            Err(failed) => {
                let at = syntax.statements.len();
                syntax.errors.push((at, failed.error.locate(original)));
                if failed.stop {
                    return syntax;
                }
                if let Some(header) = failed.ended_body {
                    ended_body = Some((None, header));
                }
            }
        }
    }
    if let Some(body) = open.body {
        let header = body.header().to_owned();
        if let Err(error) = body.close(&mut syntax, None) {
            let at = syntax.statements.len();
            syntax.errors.push((at, error.locate(&header)));
        }
    }
    syntax
}

/// What a line that parsed did to the blocks open at it.
#[derive(Default)]
struct Effect {
    /// It closed stages, the outermost of which has a header with this
    /// indentation.
    closed: Option<usize>,
    /// It ended a body, whose header has this indentation.
    ended_body: Option<usize>,
}

/// A line that failed to parse, and what reading on from it needs.
struct Failed {
    /// Boxed, so that `Result` stays small where a line parses.
    error: Box<ParseError>,
    /// Reading on as if the line were blank would not read the rest the
    /// way blanking it would.
    stop: bool,
    /// The line ended a body, whose header has this indentation.
    ended_body: Option<usize>,
}

impl Failed {
    /// A failure that left the parse as it was.
    fn clean(error: ParseError) -> Self {
        Self {
            error: Box::new(error),
            stop: false,
            ended_body: None,
        }
    }
}

/// The blocks open at a line of the flow form: its stages, and an
/// operation's body.
#[derive(Default)]
struct Open {
    stages: OpenStages,
    body: Option<OpenBody>,
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
    /// The statement whose line fixed it, if a statement did. Were that line
    /// blank, the next line of the stage would fix it instead.
    fixed_by: Option<usize>,
}

impl OpenStages {
    /// How many stages a line indented by `indent` is inside.
    fn keep(&self, indent: usize) -> usize {
        let mut keep = self.0.len();
        while keep > 0 && indent <= self.0[keep - 1].header {
            keep -= 1;
        }
        keep
    }

    /// The innermost of the first `keep` stages.
    fn name_at(&self, keep: usize) -> Option<&str> {
        let last = keep.checked_sub(1)?;
        self.0.get(last).map(|stage| stage.name.as_str())
    }

    /// Check that a line indented by `indent`, inside the first `keep`
    /// stages, lines up with the other lines of its stage. Nothing changes
    /// until [`OpenStages::enter`], so a line that fails leaves the stages
    /// as a blank line would.
    fn check(&self, keep: usize, indent: usize, number: usize) -> Result<(), ParseError> {
        let Some(stage) = keep.checked_sub(1).and_then(|last| self.0.get(last)) else {
            return Ok(());
        };
        match stage.body {
            Some(body) if body != indent => Err(ParseError::new(
                number,
                format!(
                    "this line is indented differently from the other lines of stage `{}`",
                    stage.name
                ),
            )),
            _ => Ok(()),
        }
    }

    /// The indentation of the header of the outermost stage that entering
    /// with `keep` stages would close, if it would close one.
    fn closes(&self, keep: usize) -> Option<usize> {
        self.0.get(keep).map(|stage| stage.header)
    }

    /// Whether entering with `keep` stages would fix the indentation of the
    /// stage's lines.
    fn fixes(&self, keep: usize) -> bool {
        keep.checked_sub(1)
            .is_some_and(|last| self.0[last].body.is_none())
    }

    /// The statement whose line fixed the indentation of the lines of the
    /// innermost of the first `keep` stages.
    fn fixed_by(&self, keep: usize) -> Option<usize> {
        self.0.get(keep.checked_sub(1)?)?.fixed_by
    }

    /// Close each stage that a line indented by `indent` is not inside,
    /// leaving the first `keep`, and fix the indentation of the stage's lines
    /// at the first, by `statement`, the one the line makes if it makes one.
    fn enter(&mut self, keep: usize, indent: usize, statement: Option<usize>) {
        self.0.truncate(keep);
        if let Some(stage) = self.0.last_mut() {
            if stage.body.is_none() {
                stage.body = Some(indent);
                stage.fixed_by = statement;
            }
        }
    }
}

/// Open a stage for a `stage name:` header, inside the current stage if
/// there is one. Its lines are indented beneath it, and the next line that
/// is not ends it.
fn open_stage(
    syntax: &mut Syntax,
    stages: &mut OpenStages,
    keep: usize,
    original: &str,
    declaration: &str,
    indent: usize,
    number: usize,
) -> Result<(), ParseError> {
    let syntax_error = "expected `stage name:`, with the stage's lines indented beneath it";
    if stages.name_at(keep).is_none() && indent > 0 {
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
    let full = match stages.name_at(keep) {
        Some(parent) => format!("{parent}/{name}"),
        None => name.to_owned(),
    };
    let place = name_place(original, number, declaration, name);
    stages.enter(keep, indent, Some(syntax.statements.len()));
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
        fixed_by: None,
    });
    Ok(())
}

fn flow_line(
    syntax: &mut Syntax,
    open: &mut Open,
    original: &str,
    number: usize,
) -> Result<Effect, Failed> {
    let line = strip_comment(original).trim();
    if line.is_empty() {
        return Ok(Effect::default());
    }
    let Open { stages, body } = open;
    let indent = original.len() - original.trim_start().len();
    // A line indented beneath an operation's header is a step of its body;
    // the next that is not ends it.
    let mut ended_body = None;
    if let Some(open) = body.as_mut() {
        if open.holds(indent) {
            return open
                .line(original, line, number, indent)
                .map(|()| Effect::default())
                .map_err(Failed::clean);
        }
        let ended = body.take().expect("a body is open");
        ended_body = Some(ended.header_indent());
        ended
            .close(syntax, Some(original))
            .map_err(|error| Failed {
                error: Box::new(error),
                stop: true,
                ended_body: None,
            })?;
    }
    flow_rest(syntax, stages, body, original, line, number, indent)
        .map(|closed| Effect { closed, ended_body })
        .map_err(|error| Failed {
            error: Box::new(error),
            stop: false,
            ended_body,
        })
}

/// The rest of a line that is no step of an open body. Until it succeeds it
/// changes nothing but `syntax`'s statements, as a blank line would. It
/// says whether it closed a stage, and the indentation of the outermost
/// stage's header, which a blank line would not. It also fixes the
/// indentation of a stage's lines, which a blank line would not: where a
/// later line of the stage is indented otherwise, the statement is marked
/// `stateful`.
fn flow_rest(
    syntax: &mut Syntax,
    stages: &mut OpenStages,
    body: &mut Option<OpenBody>,
    original: &str,
    line: &str,
    number: usize,
    indent: usize,
) -> Result<Option<usize>, ParseError> {
    if let Some(instead) = removed_section(line) {
        return Err(ParseError::new(
            number,
            format!("SPIT no longer reads `{line}` sections; {instead}"),
        ));
    }
    let keep = stages.keep(indent);
    if let Err(error) = stages.check(keep, indent, number) {
        // Were the line that fixed the indentation of the stage's lines
        // blank, this one would fix it, and not fail.
        if let Some(statement) = stages.fixed_by(keep) {
            syntax.statements[statement].stateful = true;
        }
        return Err(error);
    }
    let stage = stages.name_at(keep).map(str::to_owned);
    let top_level_only = |what: &str| {
        stage.as_ref().map_or(Ok(()), |name| {
            Err(ParseError::new(
                number,
                format!("{what} belongs at the top level, outside stage `{name}`"),
            ))
        })
    };
    let closed = stages.closes(keep);
    let moved = closed.is_some() || stages.fixes(keep);
    if line == "root" {
        return Err(ParseError::new(
            number,
            "expected `root <directory>`, such as `root data`",
        ));
    }
    let kind = match Keyword::split(line) {
        Some((Keyword::Stage, declaration)) => {
            return open_stage(syntax, stages, keep, original, declaration, indent, number)
                .map(|()| closed);
        }
        Some((Keyword::Sidecars, _)) => {
            return Err(ParseError::new(
                number,
                "`sidecars` is replaced by `source name : Type .ext beside sibling`; give the sibling source the path and dimensions",
            ));
        }
        Some((Keyword::Use, _)) => {
            top_level_only("`use`")?;
            StatementKind::Import
        }
        Some((Keyword::Root, folder)) => {
            top_level_only("`root`")?;
            let folder = folder.trim();
            if folder.is_empty() {
                return Err(ParseError::new(
                    number,
                    "expected `root <directory>`, such as `root data`",
                ));
            }
            StatementKind::Root(std::path::PathBuf::from(folder))
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
            let kind = StatementKind::operation(original, declaration, number, stage.clone())?;
            // A header ending in `:` opens the body of steps that carry the
            // operation out.
            if let (Some(_), StatementKind::Operation(operation, place, stage, _)) =
                (declaration.trim().strip_suffix(':'), &kind)
            {
                stages.enter(keep, indent, None);
                *body = Some(OpenBody::open(
                    operation.clone(),
                    place.clone(),
                    stage.clone(),
                    original,
                    number,
                    indent,
                    moved,
                ));
                return Ok(closed);
            }
            kind
        }
        Some((keyword @ (Keyword::Require | Keyword::Skip | Keyword::Drop), _)) => {
            top_level_only(if keyword == Keyword::Require {
                "`require`, which checks sources,"
            } else {
                "`drop`, which is replaced by conditional `exclude`,"
            })?;
            StatementKind::constraint(original, line, number)?
        }
        Some((Keyword::Exclude, declaration)) => {
            top_level_only("`exclude`, which removes sources,")?;
            let groups = declaration.trim_start();
            if groups.contains(" where ")
                || groups.contains(" per [")
                || (groups.starts_with('[')
                    && groups.split_once(']').is_some_and(|(dimensions, _)| {
                        dimensions.len() > 1 && !dimensions.contains('=')
                    }))
            {
                StatementKind::constraint(original, line, number)?
            } else {
                StatementKind::exclude(original, declaration, number)?
            }
        }
        Some((keyword @ (Keyword::Command | Keyword::Verify), declaration)) => {
            let role = if keyword == Keyword::Command {
                CommandRole::Run
            } else {
                CommandRole::Verify
            };
            StatementKind::command(original, declaration.trim(), number, role)?
        }
        Some((Keyword::Check, declaration)) => {
            StatementKind::check(original, declaration.trim(), number)?
        }
        Some((Keyword::Checks, list)) => {
            let (uses, exempt) = parse_default_checks(list.trim(), number)?;
            StatementKind::DefaultChecks {
                stage,
                checks: DefaultChecks {
                    checks: uses,
                    exempt,
                },
                place: tail_place(original, number, list.trim()),
            }
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
    stages.enter(keep, indent, Some(syntax.statements.len()));
    syntax.push(original, number, kind);
    Ok(closed)
}

/// Where a pipeline's dataset is said, for a `pipeline` line
/// written in a pipeline.
const RECIPE_HEADER: &str = "which names its pipeline and the dataset folder it is bound to; \
     a pipeline given alone names its folder with `root` or takes it from `--root`";

/// A line that starts with no keyword: a step, or a mistake.
fn flow_statement(
    original: &str,
    line: &str,
    number: usize,
    stage: Option<&str>,
) -> Result<StatementKind, ParseError> {
    if let Some(word) = unknown_keyword(line) {
        if word == "pipeline" {
            return Err(ParseError::new(
                number,
                format!("`{word}` belongs in a .spitin recipe, {RECIPE_HEADER}"),
            )
            .at_token(word));
        }
        let hint = STATEMENT_WORDS
            .iter()
            .find(|keyword| edit_distance(word, keyword) <= 2)
            .map_or_else(String::new, |keyword| format!("did you mean `{keyword}`? "));
        return Err(ParseError::new(
            number,
            format!(
                "`{word}` does not start a statement; {hint}a pipeline line starts with \
                 source, dimensions, operation, command, verify, check, path, ext, root, stage or use, or is a step \
                 `output = operation(inputs)`, and a recipe line starts with pipeline, \
                 root, discover, require, exclude or path"
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
            "expected source, discover, operation, command, verify, check, require, path, stage, or output = operation(inputs)",
        ))
    }
}

/// Parse `outputs = operation(inputs)`, where each output may declare its
/// product's type and dimensions.
pub(super) fn parse_flow_step(
    line: &str,
    number: usize,
) -> Result<(Invocation, Vec<StepOutput>), ParseError> {
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

fn parse_flow_output(left: &str, number: usize) -> Result<StepOutput, ParseError> {
    if left.contains(':') {
        let product = parse_product(left, number)?;
        if !product.checks.is_empty() {
            return Err(ParseError::new(
                number,
                "a step's product takes no `@ check(...)`; attach the check to the operation's output, as in `-> Image @ check(nonempty)`",
            )
            .at_token(left));
        }
        Ok(StepOutput {
            name: product.name,
            artifact_type: Some(product.artifact_type),
            dimensions: Some(product.dimensions),
        })
    } else {
        Ok(StepOutput {
            name: identifier(left, number, "output product")?.to_owned(),
            artifact_type: None,
            dimensions: None,
        })
    }
}

/// The words a statement can start with, for suggesting one.
const STATEMENT_WORDS: [&str; 16] = [
    "source",
    "dimensions",
    "operation",
    "command",
    "verify",
    "check",
    "path",
    "ext",
    "stage",
    "use",
    "pipeline",
    "root",
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
