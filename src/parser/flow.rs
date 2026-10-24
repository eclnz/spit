//! The flow form: `source` and `operation` declarations, steps written
//! `outputs = operation(inputs)`, and stages whose lines are indented
//! beneath a `stage name:` header.

use crate::model::{CommandRole, DefaultChecks, Invocation, TypeChecking};
use crate::span::Place;

use super::body::OpenBody;
use super::check::parse_default_checks;
use super::declarations::{
    parse_dimension_order, parse_invocation_parts, parse_path, parse_product,
    parse_type_declaration,
};
use super::entities::parse_entities;
use super::keyword::{removed_section, Keyword, BLOCKS};
use super::layout::{is_step, Block, Layout};
use super::lexical::{
    comma_items, extension, identifier, product_name, reserved_name, strip_comment,
};
use super::source_map::{name_place, step_place, tail_place};
use super::{FlowStep, ParseError, StatementKind, StepOutput, Syntax, SHELL_SOURCE_ERROR};

/// Parse the statements laid out in `layout`. Their structure, which lines make a
/// statement and which block holds each, comes from its layout, so a
/// statement that fails leaves the others reading as they would without it,
/// and one pass finds every syntax error.
///
/// A block whose opening statement fails keeps its lines, and they are read
/// in the stage around it, as ordinary statements.
pub(crate) fn parse_flow(layout: &Layout) -> Syntax {
    let mut syntax = Syntax::default();
    // For each statement that opens a stage, by index: the stage's full
    // name, once its header is read, and the indentation its lines share,
    // once the first that parses is read.
    let mut stage_names: Vec<Option<String>> = vec![None; layout.statements.len()];
    let mut stage_indents: Vec<Option<usize>> = vec![None; layout.statements.len()];
    // The operation whose body is being read, and the statement opening it.
    let mut body: Option<(usize, OpenBody)> = None;
    for (index, statement) in layout.statements.iter().enumerate() {
        if statement.blank {
            continue;
        }
        let text = layout.text(statement);
        let original = text.as_ref();
        let line = strip_comment(original).trim();
        let number = statement.lines.start + 1;
        let indent = layout.first(statement).indent;
        if let Some((opener, mut open)) = body.take() {
            if statement.parent == Some(opener) {
                if let Err(error) = open.line(original, line, number, indent) {
                    syntax.fail(error.locate(original));
                }
                body = Some((opener, open));
                continue;
            }
            let header = open.header().to_owned();
            if let Err(error) = open.close(&mut syntax, Some(original)) {
                syntax.fail(error.locate(&header));
            }
        }
        // The stage, `default:` or keyword block that holds the statement,
        // if one does; their lines share one indentation.
        let block = statement.parent.filter(|&parent| {
            matches!(
                layout.statements[parent].opens,
                Some(Block::Stage | Block::Defaults | Block::Keyword(_))
            )
        });
        let opened = block.and_then(|block| layout.statements[block].opens);
        let keyword = match opened {
            Some(Block::Keyword(keyword)) => Some(keyword),
            _ => None,
        };
        let stage = enclosing_stage(layout, &stage_names, statement.parent);
        let shared = block
            .filter(|_| !statement.stray)
            .and_then(|block| Some((block, stage_indents[block]?)));
        let read = match shared {
            Some((block, shared)) if shared != indent => {
                let opener = &layout.statements[block];
                let what = if let Some(Block::Keyword(keyword)) = opener.opens {
                    let word = BLOCKS
                        .iter()
                        .find(|&&(opens, _)| opens == keyword)
                        .map_or("", |&(_, word)| word);
                    format!("`{word}:`")
                } else if opener.opens == Some(Block::Defaults) {
                    "`default:`".to_owned()
                } else {
                    let name = stage_names[block]
                        .as_deref()
                        .unwrap_or_else(|| written_stage_name(layout.first(opener).code));
                    format!("stage `{name}`")
                };
                Err(ParseError::new(
                    number,
                    format!("this line is indented differently from the other lines of {what}"),
                ))
            }
            _ => flow_rest(
                original,
                line,
                number,
                indent,
                Within {
                    stage,
                    top_level: !in_stage_block(layout, statement.parent),
                    defaults: opened == Some(Block::Defaults),
                    block: keyword,
                },
            ),
        };
        if let (Some(block), Ok(_)) = (block, &read) {
            stage_indents[block].get_or_insert(indent);
        }
        match read {
            Ok(Read::Statement(kind)) => syntax.push(original, number, kind),
            Ok(Read::Stage { name, place }) => {
                syntax.push(
                    original,
                    number,
                    StatementKind::Stage {
                        name: name.clone(),
                        place,
                    },
                );
                stage_names[index] = Some(name);
            }
            Ok(Read::Body(open)) => body = Some((index, open)),
            Ok(Read::Header) => {}
            Err(error) => syntax.fail(error.locate(original)),
        }
    }
    if let Some((_, open)) = body {
        let header = open.header().to_owned();
        if let Err(error) = open.close(&mut syntax, None) {
            syntax.fail(error.locate(&header));
        }
    }
    syntax
}

/// The full name of the stage a statement whose block opener is `parent`
/// is in: that of the nearest stage around it whose header was read.
fn enclosing_stage<'a>(
    layout: &Layout,
    stage_names: &'a [Option<String>],
    mut parent: Option<usize>,
) -> Option<&'a str> {
    while let Some(opener) = parent {
        if let Some(name) = &stage_names[opener] {
            return Some(name);
        }
        parent = layout.statements[opener].parent;
    }
    None
}

/// Whether a statement whose block opener is `parent` is in a stage's
/// block, whether or not the stage's header was read.
fn in_stage_block(layout: &Layout, mut parent: Option<usize>) -> bool {
    while let Some(opener) = parent {
        if layout.statements[opener].opens == Some(Block::Stage) {
            return true;
        }
        parent = layout.statements[opener].parent;
    }
    false
}

/// The name a `stage` line gives, as written, for a message about the
/// lines of a stage whose header did not parse.
fn written_stage_name(code: &str) -> &str {
    let rest = code.strip_prefix("stage").unwrap_or(code).trim();
    rest.strip_suffix(':').unwrap_or(rest).trim()
}

/// What may follow `default`.
const DEFAULT_SETTINGS: &str = "a default is a `path:` rule, an `ext:` extension, a `check:` list or an `entities:` spelling of `{@entities}`, as in `default path: out/{@product}/{@entities}`";

/// What a statement read: an ordinary statement, a stage's header, the
/// header of an operation whose body follows, or the header of a block of
/// defaults or of a keyword's statements, which is no statement itself.
enum Read {
    Statement(StatementKind),
    Stage { name: String, place: Place },
    Body(OpenBody),
    Header,
}

/// One default, `setting`, written after `default` or in a `default:`
/// block: the `path:` rule, the `ext:` extension, the `check:` list or the
/// `entities` spelling of the outputs in `stage` or, outside every stage,
/// in the file.
fn default_setting(
    original: &str,
    setting: &str,
    number: usize,
    stage: Option<&str>,
) -> Result<StatementKind, ParseError> {
    let stage = stage.map(str::to_owned);
    Ok(match Keyword::split(setting) {
        Some((Keyword::Path, rest)) if setting.starts_with("path:") => {
            StatementKind::Path(parse_path(stage, original, rest, number)?)
        }
        Some((Keyword::Path, rest)) if !rest.contains(':') => {
            return Err(ParseError::new(
                number,
                "expected `:` after `default path`",
            ))
        }
        Some((Keyword::Path, _)) => {
            return Err(ParseError::new(
                number,
                "a default path rule names no product; write one product's rule as `path product: ...`, without `default`",
            ))
        }
        Some((Keyword::Ext, rest)) if setting.starts_with("ext:") => StatementKind::Extension {
            stage,
            extension: extension(rest.trim(), number)?.to_owned(),
        },
        Some((Keyword::Entities, _)) => StatementKind::Entities {
            stage,
            declaration: parse_entities(setting, number)?,
        },
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
        _ => {
            return Err(ParseError::new(
                number,
                DEFAULT_SETTINGS,
            ))
        }
    })
}

/// Read a `stage name:` header, inside `stage` if there is one. Its lines
/// are indented beneath it, and the next line that is not ends it.
fn open_stage(
    original: &str,
    declaration: &str,
    number: usize,
    indent: usize,
    stage: Option<&str>,
    top_level: bool,
) -> Result<Read, ParseError> {
    let syntax_error = "expected `stage name:`, with the stage's lines indented beneath it";
    if top_level && indent > 0 {
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
    let place = name_place(original, number, declaration, name);
    let name = match stage {
        Some(parent) => format!("{parent}/{name}"),
        None => name.to_owned(),
    };
    Ok(Read::Stage { name, place })
}

/// Where a statement is: the blocks that hold it.
struct Within<'a> {
    /// The full name of the stage it is in, if any.
    stage: Option<&'a str>,
    /// No stage's block holds it, whether or not the stage's header was read.
    top_level: bool,
    /// A `default:` block holds it.
    defaults: bool,
    /// The keyword of the keyword block that holds it, whose statements are
    /// written without it.
    block: Option<Keyword>,
}

/// Read the statement on line `number`, `original`, whose content is
/// `line`, indented by `indent`, `within` its blocks.
fn flow_rest(
    original: &str,
    line: &str,
    number: usize,
    indent: usize,
    within: Within,
) -> Result<Read, ParseError> {
    let Within {
        stage,
        top_level,
        defaults,
        block,
    } = within;
    if let Some(instead) = removed_section(line) {
        return Err(ParseError::new(
            number,
            format!("`{line}` opens no section; {instead}"),
        ));
    }
    if defaults {
        return default_setting(original, line, number, stage).map(Read::Statement);
    }
    let top_level_only = |what: &str| {
        stage.map_or(Ok(()), |name| {
            Err(ParseError::new(
                number,
                format!("{what} belongs at the top level, outside stage `{name}`"),
            ))
        })
    };
    if line == "root" {
        return Err(ParseError::new(
            number,
            "expected `root <directory>`, such as `root data`",
        ));
    }
    let split = match block {
        Some(keyword) => Some((keyword, line)),
        None => Keyword::split(line),
    };
    // A keyword alone with its `:` opens a block of its statements. Keep in
    // step with `opens` in `src/parser/layout.rs`, which gives it its lines.
    if let (None, Some((keyword, ":"))) = (block, split) {
        if BLOCKS.iter().any(|&(opener, _)| opener == keyword) {
            return Ok(Read::Header);
        }
    }
    // A keyword's word is reserved, so a step may not name its product
    // with one.
    if block.is_none() && split.is_some() && is_step(line) {
        let word = line.split([' ', '\t', ':']).next().unwrap_or(line);
        return Err(reserved_name(word, number));
    }
    let kind = match split {
        Some((Keyword::Stage, declaration)) => {
            return open_stage(original, declaration, number, indent, stage, top_level);
        }
        Some((Keyword::Sidecars, _)) => {
            return Err(ParseError::new(
                number,
                "`sidecars` starts no statement; declare each companion as `source name : Type .ext beside sibling`, and give the sibling source the path and dimensions",
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
        Some((Keyword::Types, setting)) => {
            top_level_only("`types`, which covers the whole pipeline,")?;
            StatementKind::Types(match setting.trim() {
                "strict" => TypeChecking::Strict,
                "standard" => TypeChecking::Standard,
                _ => {
                    return Err(ParseError::new(
                        number,
                        "expected `types strict` or `types standard`",
                    ))
                }
            })
        }
        Some((Keyword::Type, declaration)) => {
            top_level_only("`type`, which declares a type for the whole pipeline,")?;
            parse_type_declaration(declaration, number)?
        }
        Some((Keyword::Operation, declaration)) => {
            let kind =
                StatementKind::operation(original, declaration, number, stage.map(str::to_owned))?;
            // A header ending in `:` opens the body of steps that carry the
            // operation out. Keep in step with `opens` in
            // `src/parser/layout.rs`, which gives the body its lines.
            match (declaration.trim().ends_with(':'), kind) {
                (true, StatementKind::Operation(operation, place, stage, _)) => {
                    return Ok(Read::Body(OpenBody::open(
                        operation, place, stage, original, number,
                    )));
                }
                (_, kind) => kind,
            }
        }
        Some((keyword @ (Keyword::Require | Keyword::Skip | Keyword::Drop), rest)) => {
            // `skip` and `drop` start no rule anywhere, so their own error,
            // which gives the `exclude` to write, comes first.
            if keyword == Keyword::Require {
                top_level_only("`require`, which checks sources,")?;
            }
            StatementKind::constraint(original, keyword, rest, number)?
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
                StatementKind::constraint(original, Keyword::Exclude, declaration, number)?
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
        Some((Keyword::Default, rest)) => {
            let setting = rest.trim();
            if setting == ":" {
                return Ok(Read::Header);
            }
            default_setting(original, setting, number, stage)?
        }
        Some((Keyword::ShellSource, _)) => {
            return Err(ParseError::new(number, SHELL_SOURCE_ERROR));
        }
        Some((Keyword::Path, rest)) if block.is_some() || !line.starts_with("path:") => {
            StatementKind::Path(parse_path(
                stage.map(str::to_owned),
                original,
                rest,
                number,
            )?)
        }
        Some((Keyword::Path | Keyword::Ext | Keyword::Checks | Keyword::Entities, _)) => {
            let setting = line.split_once(':').map_or(line, |(word, _)| word);
            return Err(ParseError::new(
                number,
                format!(
                    "a default is written after `default`, as `default {setting}: ...`, or in a `default:` block"
                ),
            )
            .at_token(setting));
        }
        None => flow_statement(original, line, number, stage)?,
    };
    Ok(Read::Statement(kind))
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
        if word == "default" {
            return Err(ParseError::new(number, DEFAULT_SETTINGS).at_token(word));
        }
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
                 source, dimensions, types, operation, command, verify, check, path, default, root, stage or use, or is a step \
                 `output = operation(inputs)`, and a recipe line starts with pipeline, \
                 root, inputs, require, exclude or default"
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
            "expected source, operation, command, verify, check, require, path, default, stage, or output = operation(inputs)",
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
                "a step's product takes no `@check(...)`; attach the check to the operation's output, as in `-> Image @check(nonempty)`",
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
            name: product_name(left, number, "output product")?.to_owned(),
            artifact_type: None,
            dimensions: None,
        })
    }
}

/// The words a statement can start with, for suggesting one.
const STATEMENT_WORDS: [&str; 16] = [
    "source",
    "dimensions",
    "types",
    "operation",
    "command",
    "verify",
    "check",
    "path",
    "default",
    "stage",
    "use",
    "pipeline",
    "root",
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
            unknown_keyword("mean : Image [s] = average(x @vary(r))"),
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
