//! Errors the parser reads from `recipe`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `recipe`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecipeError {
    /// Says: a .spitin names its pipeline once
    PipelineNamedTwice,
    /// Says: a .spitin names its {what} once
    HeaderNamedTwice { what: String },
    /// Says: input paths must use the same indentation
    InputIndentationDiffers,
    /// Says: a recipe has one `inputs:` block
    InputsBlockRepeated,
    /// Says: put source paths and contexts in the `inputs:` block
    PathsOutsideInputsBlock,
    /// Says: `inputs:` needs an indented path entry
    InputsBlockEmpty,
    /// Says: expected `source: path`, `default: path`, or `contexts: directory-pattern`
    ExpectedInputEntry,
    /// Says: input path must not be empty
    EmptyInputPath,
    /// Says: duplicate input path `{name}`
    DuplicateInputPath { name: String },
    /// Says: a default `ext:` completes the pipeline's default output paths; it belongs in the .spit pipeline
    DefaultExtInRecipe,
    /// Says: `{word}` belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `inputs:`, `exclude` and `require` rules
    BelongsInPipeline { word: String },
    /// Says: a step belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `inputs:`, `exclude` and `require` rules
    StepInRecipe,
    /// Says: `source_paths:` belongs in a .spitout; use `path image:` in a .spitin
    SourcePathsInRecipe,
    /// Says: a .spitin file may contain discovery, coverage, source paths, and inventory records only
    RecipeHoldsOtherContent,
    /// Says: a .spitin `inputs: default:` is for sources, and no source is made in a stage; leave out `{@stage}`
    StageInInputsDefault,
    /// Says: cannot read `{file}` for `review {review}`: {reason}
    CannotReadReview {
        file: String,
        review: String,
        reason: String,
    },
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PipelineNamedTwice => f.write_str("a .spitin names its pipeline once"),
            Self::HeaderNamedTwice { what } => write!(f, "a .spitin names its {what} once"),
            Self::InputIndentationDiffers => f.write_str("input paths must use the same indentation"),
            Self::InputsBlockRepeated => f.write_str("a recipe has one `inputs:` block"),
            Self::PathsOutsideInputsBlock => f.write_str("put source paths and contexts in the `inputs:` block"),
            Self::InputsBlockEmpty => f.write_str("`inputs:` needs an indented path entry"),
            Self::ExpectedInputEntry => f.write_str("expected `source: path`, `default: path`, or `contexts: directory-pattern`"),
            Self::EmptyInputPath => f.write_str("input path must not be empty"),
            Self::DuplicateInputPath { name } => write!(f, "duplicate input path `{name}`"),
            Self::DefaultExtInRecipe => f.write_str("a default `ext:` completes the pipeline's default output paths; it belongs in the .spit pipeline"),
            Self::BelongsInPipeline { word } => write!(f, "`{word}` belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `inputs:`, `exclude` and `require` rules"),
            Self::StepInRecipe => write!(f, "a step belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `inputs:`, `exclude` and `require` rules"),
            Self::SourcePathsInRecipe => f.write_str("`source_paths:` belongs in a .spitout; use `path image:` in a .spitin"),
            Self::RecipeHoldsOtherContent => f.write_str("a .spitin file may contain discovery, coverage, source paths, and inventory records only"),
            Self::StageInInputsDefault => f.write_str("a .spitin `inputs: default:` is for sources, and no source is made in a stage; leave out `{@stage}`"),
            Self::CannotReadReview { file, review, reason } => write!(f, "cannot read `{file}` for `review {review}`: {reason}"),
        }
    }
}
