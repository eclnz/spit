use std::error::Error;
use std::fmt;

use crate::model::ResolveError;
use crate::template::PathError;

use super::{EveryGroupDropped, UnmatchedExclusion};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputError {
    RootInBoth,
    OutputInBoth,
    Resolve(ResolveError),
    Path(PathError),
    NotASource {
        product: String,
    },
    OutputPath {
        product: String,
    },
    PathInBoth {
        product: String,
    },
    MemberPath {
        product: String,
        group: String,
    },
    NoSourcePath {
        product: String,
    },
    ConflictingSourcePaths {
        product: String,
    },
    UnknownSourcePath {
        product: String,
    },
    InventoryPathInBoth {
        product: String,
    },
    UnmatchedExclusion(UnmatchedExclusion),
    EveryGroupDropped(EveryGroupDropped),
    MissingTool {
        tool: String,
        file: String,
        line: usize,
    },
}

impl From<ResolveError> for InputError {
    fn from(error: ResolveError) -> Self {
        Self::Resolve(error)
    }
}

impl From<PathError> for InputError {
    fn from(error: PathError) -> Self {
        Self::Path(error)
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTool { tool, file, line } => write!(
                f,
                "the pipeline declares tool `{tool}`, which `{file}` (named on line {line}) does not list; add a `{tool}:` line to it"
            ),
            Self::RootInBoth => f.write_str("dataset root is declared in both .spit and .spitin; keep one declaration"),
            Self::OutputInBoth => f.write_str("output root is declared in both .spit and .spitin; keep one declaration"),
            Self::Resolve(error) => error.fmt(f),
            Self::Path(error) => error.fmt(f),
            Self::NotASource { product } => write!(
                f,
                "input path `{product}` must name a source product in the pipeline"
            ),
            Self::OutputPath { product } => write!(
                f,
                "`{product}` is made by a step, so its path belongs in the .spit pipeline; a .spitin gives paths only for sources"
            ),
            Self::PathInBoth { product } => write!(
                f,
                "`{product}` has path rules in both .spit and .spitin; keep the pipeline's if every dataset has this layout, or the recipe's if only this one does"
            ),
            Self::MemberPath { product, group } => write!(
                f,
                "source `{product}` is beside `{group}` and takes its path from it; give `{group}` the path in `inputs:` instead"
            ),
            Self::NoSourcePath { product } => write!(
                f,
                "source `{product}` has no path rule, so its files cannot be found; write `path {product}:` in the pipeline or `{product}:` or `default:` in the recipe's `inputs:` block"
            ),
            Self::ConflictingSourcePaths { product } => write!(
                f,
                "source `{product}` has conflicting path rules in .spitin and .spitout"
            ),
            Self::UnknownSourcePath { product } => {
                write!(f, "source path rule names unknown source `{product}`")
            }
            Self::InventoryPathInBoth { product } => write!(
                f,
                "source `{product}` has path rules in both .spit and .spitout"
            ),
            Self::EveryGroupDropped(emptied) => emptied.fmt(f),
            Self::UnmatchedExclusion(unmatched) => {
                write!(
                    f,
                    "`{}` ({}) matches nothing in this dataset",
                    unmatched.rule, unmatched.origin
                )?;
                if !unmatched.near.is_empty() {
                    write!(f, "; it has {}", unmatched.near.join(", "))?;
                }
                Ok(())
            }
        }
    }
}

impl Error for InputError {}
