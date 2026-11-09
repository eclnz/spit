//! Path rules as patterns for discovery: a source's rule compiled to match,
//! where a pipeline's outputs lie, and how near a file comes to a rule that
//! matches none.

use std::collections::BTreeSet;
use std::fmt;

use rustc_hash::FxHashSet;

use crate::model::stage_directories;
use crate::model::{Pipeline, PipelineIndex, ProductDef};
use crate::paths::unspelled_entities;
use crate::template::{error, Gap, PathError, PathPattern, PathPlaceholder, PathTemplate};

/// `product`'s path pattern, for matching. `stage` is the directories
/// `{@stage}` gives it, `None` for a source.
pub(super) fn path_pattern(
    template: &PathTemplate,
    product: &ProductDef,
    stage: Option<&str>,
) -> Result<PathPattern, PathError> {
    let pattern = PathPattern::new(template, &product.name, stage);
    match pattern.gap {
        None => Ok(pattern),
        // A source with dimensions and a spelling has `{@entities}`
        // written out; the path checks fail the rest first.
        Some(Gap::Entities) => Err(error(unspelled_entities(
            &product.name,
            product.dimensions.is_empty(),
        ))),
        // A source is made in no stage, so its rule never binds `{@stage}`;
        // `inspect_paths` rejects such a rule first.
        Some(Gap::Stage) => Err(error(format!(
            "path rule for `{}` uses `{}`, but `{}` is not made in a stage",
            product.name,
            PathPlaceholder::Stage,
            product.name
        ))),
    }
}

/// Where the products a step of a pipeline makes lie under a root, to tell
/// what an earlier run wrote from files no rule matches.
pub(super) struct OutputPaths<'l> {
    /// The pattern of each file output.
    files: Vec<PathPattern>,
    /// The pattern of each file output written beside a source's file.
    beside_sources: Vec<PathPattern>,
    /// The directories a folder output's pattern matches.
    folders: FxHashSet<&'l str>,
}

impl<'l> OutputPaths<'l> {
    /// Each output's completed path template, with `{@stage}` written out
    /// as binding does. An output whose path cannot be bound is left out:
    /// resolving reports it.
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

    /// Whether `file` is at the path of an output written beside a source's
    /// file, which is neither a source nor a file no rule matches.
    pub(super) fn beside_source(&self, file: &str) -> bool {
        self.beside_sources
            .iter()
            .any(|pattern| pattern.match_path(file).is_some())
    }

    /// Whether `file` is at an output's path, or inside an output folder.
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

/// A source whose path rule matched no file under the scanned root, and the
/// file left unmatched that comes nearest to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissedSource {
    pub product: String,
    /// Whether the source reads folders, so its nearest is a folder.
    pub folder: bool,
    /// The rule as it was matched, with `{@product}` and `{@entities}`
    /// written out and its extension added.
    pub rule: String,
    pub nearest: Option<NearestFile>,
}

/// Where a file parts from a rule it does not match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NearestFile {
    pub file: String,
    /// How many bytes at the start of `file` the rule matches.
    pub matched: usize,
    /// The rest of the rule from there, empty when the file goes on past
    /// the rule's end.
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
    /// Where the file parts from the rule, as "after `sub-01_`, the file
    /// has `acq-2_T1w` where the rule has `T1w`"; `kind` is `file` or
    /// `folder`.
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

/// `product`'s rule, `pattern`, which matched no file, with the file of
/// `files` it comes nearest to: the one it matches furthest from the start,
/// then the one that shares most of the rule's ending. A file is near only
/// when the rule matches some of its start, or it ends with the rule's last
/// literal, such as `_bold.nii.gz`, in full. A `folder` source's `files`
/// are the folders under the root.
pub(super) fn missed_source(
    product: &str,
    pattern: &PathPattern,
    files: &[String],
    folder: bool,
) -> MissedSource {
    let ending = pattern.ending();
    let shared_ending = |file: &str| {
        file.bytes()
            .rev()
            .zip(ending.bytes().rev())
            .take_while(|(a, b)| a == b)
            .count()
    };
    let mut best: Option<(usize, usize, NearestFile)> = None;
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
    MissedSource {
        product: product.to_owned(),
        folder,
        rule: pattern.to_string(),
        nearest: best.map(|(_, _, near)| near),
    }
}
