use std::collections::BTreeSet;
use std::fmt;

use rustc_hash::FxHashSet;

use crate::model::stage_directories;
use crate::model::{Pipeline, PipelineIndex, ProductDef};
use crate::paths::unspelled_entities;
use crate::template::{error, Gap, PathError, PathPattern, PathPlaceholder, PathTemplate};

pub(super) fn path_pattern(
    template: &PathTemplate,
    product: &ProductDef,
    stage: Option<&str>,
) -> Result<PathPattern, PathError> {
    let pattern = PathPattern::new(template, &product.name, stage);
    match pattern.gap {
        None => Ok(pattern),
        Some(Gap::Entities) => Err(error(unspelled_entities(
            &product.name,
            product.dimensions.is_empty(),
        ))),
        Some(Gap::Stage) => Err(error(format!(
            "path rule for `{}` uses `{}`, but `{}` is not made in a stage",
            product.name,
            PathPlaceholder::Stage,
            product.name
        ))),
    }
}

pub(super) struct OutputPaths<'l> {
    files: Vec<PathPattern>,
    beside_sources: Vec<PathPattern>,
    folders: FxHashSet<&'l str>,
}

impl<'l> OutputPaths<'l> {
    pub(super) fn of(pipeline: &Pipeline, directories: &'l [String]) -> Self {
        let index = PipelineIndex::new(pipeline);
        let outputs: BTreeSet<_> = pipeline
            .invocations
            .iter()
            .flat_map(|invocation| &invocation.outputs)
            .collect();
        let mut files = Vec::new();
        let mut beside_sources = Vec::new();
        let mut folder_patterns = Vec::new();
        for product in &pipeline.products {
            if !outputs.contains(&product.name) {
                continue;
            }
            let Some(template) = index.path_template_for(&product.name) else {
                continue;
            };
            let stage = stage_directories(&index, &product.name);
            let Ok(pattern) = path_pattern(&template, product, stage.as_deref()) else {
                continue;
            };
            if index.is_folder(&product.name) {
                folder_patterns.push(pattern);
            } else {
                let beside_source = index
                    .beside(&product.name)
                    .is_some_and(|beside| index.is_source(beside.sibling));
                if beside_source {
                    beside_sources.push(pattern);
                } else {
                    files.push(pattern);
                }
            }
        }
        let folders = directories
            .iter()
            .filter(|directory| {
                folder_patterns
                    .iter()
                    .any(|pattern| pattern.match_path(directory).is_some())
            })
            .map(String::as_str)
            .collect();
        Self {
            files,
            beside_sources,
            folders,
        }
    }

    pub(super) fn beside_source(&self, file: &str) -> bool {
        self.beside_sources
            .iter()
            .any(|pattern| pattern.match_path(file).is_some())
    }

    pub(super) fn hold(&self, file: &str) -> bool {
        self.files
            .iter()
            .any(|pattern| pattern.match_path(file).is_some())
            || self.beside_source(file)
            || file
                .match_indices('/')
                .any(|(end, _)| self.folders.contains(&file[..end]))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissedSource {
    pub product: String,
    pub folder: bool,
    pub rule: String,
    pub nearest: Option<NearestFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NearestFile {
    pub file: String,
    pub matched: usize,
    pub expected: String,
}

impl fmt::Display for MissedSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.folder { "folder" } else { "file" };
        write!(
            f,
            "source `{}` matched no {kind}s with path rule `{}`",
            self.product, self.rule
        )?;
        let Some(near) = &self.nearest else {
            return Ok(());
        };
        write!(
            f,
            "\n  the nearest {kind} is `{}`\n  {}",
            near.file,
            near.parting(kind)
        )
    }
}

impl NearestFile {
    pub fn parting(&self, kind: &str) -> String {
        let (start, rest) = self.file.split_at(self.matched);
        let after = if start.is_empty() {
            String::new()
        } else {
            format!("after `{start}`, ")
        };
        let expected = &self.expected;
        match (rest.is_empty(), expected.is_empty()) {
            (true, _) => format!("{after}the {kind} ends where the rule has `{expected}`"),
            (false, true) => format!("{after}the {kind} has `{rest}` where the rule ends"),
            (false, false) => {
                format!("{after}the {kind} has `{rest}` where the rule has `{expected}`")
            }
        }
    }
}

pub(super) fn missed_source(
    product: &str,
    patterns: &[PathPattern],
    files: &[String],
    folder: bool,
) -> MissedSource {
    let mut best: Option<(usize, usize, NearestFile)> = None;
    for pattern in patterns {
        let ending = pattern.ending();
        let shared_ending = |file: &str| {
            file.bytes()
                .rev()
                .zip(ending.bytes().rev())
                .take_while(|(a, b)| a == b)
                .count()
        };
        for file in files {
            let (matched, expected) = pattern.reach(file);
            let score = (matched, shared_ending(file));
            let near = matched > 0 || (!ending.is_empty() && score.1 == ending.len());
            if near && best.as_ref().is_none_or(|(a, b, _)| score > (*a, *b)) {
                let near = NearestFile {
                    file: file.clone(),
                    matched,
                    expected,
                };
                best = Some((score.0, score.1, near));
            }
        }
    }
    MissedSource {
        product: product.to_owned(),
        folder,
        rule: patterns
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("` or `"),
        nearest: best.map(|(_, _, near)| near),
    }
}
