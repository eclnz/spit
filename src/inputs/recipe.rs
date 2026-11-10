//! The recipe's own lines: its `pipeline`, `root` and `output` lines, and
//! its `inputs:` block, lowered to common path statements and context
//! patterns while preserving line numbers for diagnostics.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::PathBuf;

use rustc_hash::FxHashSet;

use crate::model::ContextPattern;
use crate::parser::{Layout, ParseError, SourceMap};
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
                return Err(ParseError::new(number, "a .spitin names its pipeline once"));
            }
            header.pipeline = Some(PathBuf::from(file.trim()));
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
                return Err(ParseError::new(
                    number,
                    format!("a .spitin names its {what} once"),
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
    AwaitingEntry { header: usize, line: usize },
    Reading { header: usize, body: usize },
    After,
}

impl Block {
    /// A nonblank line either belongs to the block or closes it.
    fn contains(&mut self, indent: usize, number: usize) -> Result<bool, ParseError> {
        match *self {
            Self::AwaitingEntry { header, .. } if indent > header => {
                *self = Self::Reading {
                    header,
                    body: indent,
                };
                Ok(true)
            }
            Self::AwaitingEntry { line, .. } => Err(empty_block(line)),
            Self::Reading { header, body } if indent > header && indent != body => Err(
                ParseError::new(number, "input paths must use the same indentation"),
            ),
            Self::Reading { header, .. } if indent > header => Ok(true),
            Self::Reading { .. } => {
                *self = Self::After;
                Ok(false)
            }
            Self::Before | Self::After => Ok(false),
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
        if block.contains(indent, number)? {
            write_entry(line, number, &mut names, &mut contexts, &mut out)?;
            continue;
        }
        match (line, &block) {
            ("inputs:", Block::Before) => {
                block = Block::AwaitingEntry {
                    header: indent,
                    line: number,
                };
                out.push('\n');
            }
            ("inputs:", _) => {
                return Err(ParseError::new(number, "a recipe has one `inputs:` block"))
            }
            _ if line.starts_with("discover ")
                || line.starts_with("path ")
                || line.starts_with("path:")
                || line.starts_with("default path:") =>
            {
                return Err(ParseError::new(
                    number,
                    "put source paths and contexts in the `inputs:` block",
                ));
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
    ParseError::new(line, "`inputs:` needs an indented path entry")
}

fn write_entry(
    line: &str,
    number: usize,
    names: &mut FxHashSet<String>,
    contexts: &mut Vec<ContextPattern>,
    out: &mut String,
) -> Result<(), ParseError> {
    let (name, path) = line.split_once(": ").ok_or_else(|| {
        ParseError::new(
            number,
            "expected `source: path`, `default: path`, or `contexts: directory-pattern`",
        )
    })?;
    match (name, path) {
        (_, "") => return Err(ParseError::new(number, "input path must not be empty")),
        ("contexts", path) => {
            contexts.push(context_pattern(path, number)?);
            out.push('\n');
        }
        (name, _) if !names.insert(name.to_owned()) => {
            return Err(ParseError::new(
                number,
                format!("duplicate input path `{name}`"),
            ));
        }
        ("default", path) => {
            writeln!(out, "default path: {path}").expect("writing to a String succeeds")
        }
        (name, path) => writeln!(out, "path {name}: {path}").expect("writing to a String succeeds"),
    }
    Ok(())
}

fn context_pattern(path: &str, number: usize) -> Result<ContextPattern, ParseError> {
    let template =
        PathTemplate::parse(path).map_err(|error| ParseError::new(number, error.message()))?;
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
    validate_context_pattern(&context).map_err(|error| ParseError::new(number, error.message()))?;
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
