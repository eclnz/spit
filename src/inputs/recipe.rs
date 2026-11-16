//! The recipe's own lines: its `pipeline`, `root`, `output` and `tools from` lines, and
//! its `inputs:` block, lowered to common path statements and context
//! patterns while preserving line numbers for diagnostics.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::PathBuf;

use rustc_hash::FxHashSet;

use crate::model::ContextPattern;
use crate::parser::error::RecipeError;
use crate::parser::{Layout, ParseError, ParseErrorKind, SourceMap};
use crate::paths::validate_context_pattern;
use crate::span::strip_comment;
use crate::template::{PathPart, PathPlaceholder, PathTemplate};

/// The folders a recipe's `pipeline`, `root` and `output` lines name, as
/// written.
#[derive(Default)]
pub(super) struct RecipeHeader {
    pub(super) pipeline: Option<PathBuf>,
    pub(super) root: Option<(PathBuf, usize)>,
    pub(super) output: Option<(PathBuf, usize)>,
    /// The `.spitenv` a `tools from` line names, as written, and its line.
    pub(super) tools: Option<(String, usize)>,
}

/// The pipeline a recipe names with `pipeline analysis.spit`, the root it
/// names with `root data` and the output root it names with `output
/// results`, and the text with those lines blanked so that other lines keep
/// their numbers.
pub(super) fn header_lines(text: &str) -> Result<(RecipeHeader, String), ParseError> {
    let mut header = RecipeHeader::default();
    let mut rest = String::new();
    for (number, line) in Layout::new(text).numbered() {
        if let Some(file) = line.code.strip_prefix("pipeline ") {
            if header.pipeline.is_some() {
                return Err(ParseError::of(number, RecipeError::PipelineNamedTwice));
            }
            header.pipeline = Some(PathBuf::from(file.trim()));
        } else if let Some(rest) = line.code.strip_prefix("tools ") {
            let file = rest.trim_start().strip_prefix("from ").map(str::trim);
            let Some(file) = file.filter(|file| !file.is_empty()) else {
                return Err(ParseError::of(number, RecipeError::ExpectedToolsFrom));
            };
            if header.tools.is_some() {
                return Err(ParseError::of(number, RecipeError::ToolsFromRepeated));
            }
            header.tools = Some((file.to_owned(), number));
        } else if let Some((folder, slot, what)) = line
            .code
            .strip_prefix("root ")
            .map(|folder| (folder, &mut header.root, "root"))
            .or_else(|| {
                let folder = line.code.strip_prefix("output ")?;
                Some((folder, &mut header.output, "output root"))
            })
        {
            if slot.is_some() {
                return Err(ParseError::of(
                    number,
                    RecipeError::HeaderNamedTwice {
                        what: what.to_string(),
                    },
                ));
            }
            *slot = Some((PathBuf::from(folder.trim()), number));
        } else {
            rest.push_str(line.text);
        }
        rest.push('\n');
    }
    Ok((header, rest))
}

enum Block {
    Before,
    AwaitingEntry {
        header: usize,
        line: usize,
    },
    /// `rules`: the last entry named a source alone, as `t1:`, so its rules
    /// are on the lines indented beneath it.
    Reading {
        header: usize,
        body: usize,
        rules: bool,
    },
    After,
}

/// Where a line of the `inputs:` block belongs.
enum Entry {
    /// An entry of its own, as `t1: raw/{id}.nii`.
    Own,
    /// A rule of the entry above it.
    Rule,
}

impl Block {
    /// A nonblank line either belongs to the block, as an entry or a rule of
    /// the one above it, or closes it.
    fn contains(&mut self, indent: usize, number: usize) -> Result<Option<Entry>, ParseError> {
        match *self {
            Self::AwaitingEntry { header, .. } if indent > header => {
                *self = Self::Reading {
                    header,
                    body: indent,
                    rules: false,
                };
                Ok(Some(Entry::Own))
            }
            Self::AwaitingEntry { line, .. } => Err(empty_block(line)),
            Self::Reading {
                body, rules: true, ..
            } if indent > body => Ok(Some(Entry::Rule)),
            Self::Reading { header, body, .. } if indent > header && indent != body => {
                Err(ParseError::of(number, RecipeError::InputIndentationDiffers))
            }
            Self::Reading { header, .. } if indent > header => Ok(Some(Entry::Own)),
            Self::Reading { .. } => {
                *self = Self::After;
                Ok(None)
            }
            Self::Before | Self::After => Ok(None),
        }
    }
}

pub(super) fn expand_input_block(text: &str) -> Result<(String, Vec<ContextPattern>), ParseError> {
    let mut out = String::new();
    let mut block = Block::Before;
    let mut names = FxHashSet::default();
    let mut contexts = Vec::new();
    for (index, original) in text.lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(original).trim();
        let indent = original.len() - original.trim_start().len();
        if line.is_empty() {
            out.push('\n');
            continue;
        }
        match block.contains(indent, number)? {
            Some(Entry::Own) => {
                let rules = write_entry(line, number, &mut names, &mut contexts, &mut out)?;
                if let Block::Reading { rules: open, .. } = &mut block {
                    *open = rules;
                }
                continue;
            }
            // Kept indented, beneath the `path` line its entry became.
            Some(Entry::Rule) => {
                out.push_str(original);
                out.push('\n');
                continue;
            }
            None => {}
        }
        match (line, &block) {
            ("inputs:", Block::Before) => {
                block = Block::AwaitingEntry {
                    header: indent,
                    line: number,
                };
                out.push('\n');
            }
            ("inputs:", _) => return Err(ParseError::of(number, RecipeError::InputsBlockRepeated)),
            _ if line.starts_with("discover ")
                || line.starts_with("path ")
                || line.starts_with("path:")
                || line.starts_with("default path:") =>
            {
                return Err(ParseError::of(number, RecipeError::PathsOutsideInputsBlock));
            }
            _ => {
                out.push_str(original);
                out.push('\n');
            }
        }
    }
    match block {
        Block::AwaitingEntry { line, .. } => Err(empty_block(line)),
        _ => Ok((out, contexts)),
    }
}

fn empty_block(line: usize) -> ParseError {
    ParseError::of(line, RecipeError::InputsBlockEmpty)
}

/// Write the entry on `line` to `out` as a path statement. Returns whether
/// it names a source alone, as `t1:`, whose rules are on the lines beneath.
fn write_entry(
    line: &str,
    number: usize,
    names: &mut FxHashSet<String>,
    contexts: &mut Vec<ContextPattern>,
    out: &mut String,
) -> Result<bool, ParseError> {
    let alone = line
        .strip_suffix(':')
        .filter(|name| !name.contains([' ', ':']));
    let (name, path) = match alone {
        Some(name) => (name, ""),
        None => line
            .split_once(": ")
            .ok_or_else(|| ParseError::of(number, RecipeError::ExpectedInputEntry))?,
    };
    match (name, path) {
        ("contexts" | "default", "") => {
            return Err(ParseError::of(number, RecipeError::EmptyInputPath))
        }
        (name, "") if !names.insert(name.to_owned()) => {
            return Err(ParseError::of(
                number,
                RecipeError::DuplicateInputPath {
                    name: name.to_string(),
                },
            ));
        }
        (name, "") => {
            writeln!(out, "path {name}:").expect("writing to a String succeeds");
            return Ok(true);
        }
        ("contexts", path) => {
            contexts.push(context_pattern(path, number)?);
            out.push('\n');
        }
        (name, _) if !names.insert(name.to_owned()) => {
            return Err(ParseError::of(
                number,
                RecipeError::DuplicateInputPath {
                    name: name.to_string(),
                },
            ));
        }
        ("default", path) => {
            writeln!(out, "default path: {path}").expect("writing to a String succeeds")
        }
        (name, path) => writeln!(out, "path {name}: {path}").expect("writing to a String succeeds"),
    }
    Ok(false)
}

fn context_pattern(path: &str, number: usize) -> Result<ContextPattern, ParseError> {
    let template = PathTemplate::parse(path)
        .map_err(|error| ParseError::of(number, ParseErrorKind::PathTemplate(error.error)))?;
    let dimensions = template
        .parts()
        .iter()
        .filter_map(|part| match part {
            PathPart::Placeholder(PathPlaceholder::Dimension(dimension, _)) => {
                Some(dimension.clone())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let context = ContextPattern {
        dimensions,
        template,
    };
    validate_context_pattern(&context)
        .map_err(|error| ParseError::of(number, ParseErrorKind::PathTemplate(error.error)))?;
    Ok(context)
}

/// Path parsing uses lowered statements; diagnostics point into the user's
/// indented entries instead. Update only the declarations on the same line.
pub(super) fn restore_path_places(text: &str, places: &mut SourceMap) {
    for (index, original) in text.lines().enumerate() {
        let Some((name, path)) = original.split_once(": ") else {
            continue;
        };
        let place = match name.trim() {
            "default" => places.default_path.as_mut(),
            name => places.paths.get_mut(name),
        };
        match place {
            Some(place) if place.line == index + 1 => {
                place.columns =
                    original.len() - path.len()..strip_comment(original).trim_end().len();
            }
            _ => {}
        }
    }
}
