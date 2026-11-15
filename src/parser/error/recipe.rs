//! The errors of reading a `.spitin`: its header and `inputs:` block; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `recipe`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecipeError {
    /// Reads: a .spitin names its pipeline once
    PipelineNamedTwice,
    /// Reads: a .spitin names its {what} once
    HeaderNamedTwice { what: String },
    /// Reads: input paths must use the same indentation
    InputIndentationDiffers,
    /// Reads: a recipe has one `inputs:` block
    InputsBlockRepeated,
    /// Reads: put source paths and contexts in the `inputs:` block
    PathsOutsideInputsBlock,
    /// Reads: `inputs:` needs an indented path entry
    InputsBlockEmpty,
    /// Reads: expected `source: path`, `default: path`, or `contexts: directory-pattern`
    ExpectedInputEntry,
    /// Reads: input path must not be empty
    EmptyInputPath,
    /// Reads: duplicate input path `{name}`
    DuplicateInputPath { name: String },
    /// Reads: a default `ext:` completes the pipeline's default output paths; it belongs in the .spit pipeline
    DefaultExtInRecipe,
    /// Reads: `{word}` belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `inputs:`, `exclude` and `require` rules
    BelongsInPipeline { word: String },
    /// Reads: a step belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `inputs:`, `exclude` and `require` rules
    StepInRecipe,
    /// Reads: `source_paths:` belongs in a .spitout; use `path image:` in a .spitin
    SourcePathsInRecipe,
    /// Reads: a .spitin file may contain discovery, coverage, source paths, and inventory records only
    RecipeHoldsOtherContent,
    /// Reads: a .spitin `inputs: default:` is for sources, and no source is made in a stage; leave out `{@stage}`
    StageInInputsDefault,
    /// Reads: cannot read `{file}` for `review {review}`: {reason}
    CannotReadReview {
        file: String,
        review: String,
        reason: String,
    },
    /// Reads: a .spitenv is one `tools:` block, with a `name: identity` line indented beneath it for each tool, as in `fsl: probe flirt -version`
    SpitenvForm,
    /// Reads: `{name}` is not a tool name: use letters, digits and `_`, starting with a letter
    BadToolName { name: String },
    /// Reads: tool `{name}` is listed twice
    ToolRepeated { name: String },
    /// Reads: say what `{name}` is: a text such as a container digest, or `probe` and a command whose output identifies it
    ToolWithoutIdentity { name: String },
    /// Reads: `{name}`'s probe: {message}
    ProbeProblem { name: String, message: String },
    /// Reads: `{name}`'s probe runs with no shell, so `<`, `>` and `|` are not its; run `sh -c '...'` for a pipeline of commands
    ProbeUsesShellOperator { name: String },
    /// Reads: `{name}`'s probe is run as written, so it takes no `{...}` or `@scalar`; quote a brace to pass it
    ProbeUsesPlaceholder { name: String },
    /// Reads: cannot read `{file}` for `tools from`: {reason}
    CannotReadSpitenv { file: String, reason: String },
    /// Reads: {file} line {line}: {message}
    SpitenvLine {
        file: String,
        line: String,
        message: String,
    },
    /// Reads: expected `tools from file.spitenv`, the file that says what each tool is
    ExpectedToolsFrom,
    /// Reads: a .spitin names its tools once
    ToolsFromRepeated,
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
            Self::SpitenvForm => f.write_str("a .spitenv is one `tools:` block, with a `name: identity` line indented beneath it for each tool, as in `fsl: probe flirt -version`"),
            Self::BadToolName { name } => write!(f, "`{name}` is not a tool name: use letters, digits and `_`, starting with a letter"),
            Self::ToolRepeated { name } => write!(f, "tool `{name}` is listed twice"),
            Self::ToolWithoutIdentity { name } => write!(f, "say what `{name}` is: a text such as a container digest, or `probe` and a command whose output identifies it"),
            Self::ProbeProblem { name, message } => write!(f, "`{name}`'s probe: {message}"),
            Self::ProbeUsesShellOperator { name } => write!(f, "`{name}`'s probe runs with no shell, so `<`, `>` and `|` are not its; run `sh -c '...'` for a pipeline of commands"),
            Self::ProbeUsesPlaceholder { name } => write!(f, "`{name}`'s probe is run as written, so it takes no `{{...}}` or `@scalar`; quote a brace to pass it"),
            Self::CannotReadSpitenv { file, reason } => write!(f, "cannot read `{file}` for `tools from`: {reason}"),
            Self::SpitenvLine { file, line, message } => write!(f, "{file} line {line}: {message}"),
            Self::ExpectedToolsFrom => f.write_str("expected `tools from file.spitenv`, the file that says what each tool is"),
            Self::ToolsFromRepeated => f.write_str("a .spitin names its tools once"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant names each text it holds in its wording.
    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            RecipeError::PipelineNamedTwice,
            RecipeError::HeaderNamedTwice {
                what: "what".to_owned(),
            },
            RecipeError::InputIndentationDiffers,
            RecipeError::InputsBlockRepeated,
            RecipeError::PathsOutsideInputsBlock,
            RecipeError::InputsBlockEmpty,
            RecipeError::ExpectedInputEntry,
            RecipeError::EmptyInputPath,
            RecipeError::DuplicateInputPath {
                name: "name".to_owned(),
            },
            RecipeError::DefaultExtInRecipe,
            RecipeError::BelongsInPipeline {
                word: "word".to_owned(),
            },
            RecipeError::StepInRecipe,
            RecipeError::SourcePathsInRecipe,
            RecipeError::RecipeHoldsOtherContent,
            RecipeError::StageInInputsDefault,
            RecipeError::CannotReadReview {
                file: "file".to_owned(),
                review: "review".to_owned(),
                reason: "reason".to_owned(),
            },
            RecipeError::SpitenvForm,
            RecipeError::BadToolName {
                name: "name".to_owned(),
            },
            RecipeError::ToolRepeated {
                name: "name".to_owned(),
            },
            RecipeError::ToolWithoutIdentity {
                name: "name".to_owned(),
            },
            RecipeError::ProbeProblem {
                name: "name".to_owned(),
                message: "message".to_owned(),
            },
            RecipeError::ProbeUsesShellOperator {
                name: "name".to_owned(),
            },
            RecipeError::ProbeUsesPlaceholder {
                name: "name".to_owned(),
            },
            RecipeError::CannotReadSpitenv {
                file: "file".to_owned(),
                reason: "reason".to_owned(),
            },
            RecipeError::SpitenvLine {
                file: "file".to_owned(),
                line: "line".to_owned(),
                message: "message".to_owned(),
            },
            RecipeError::ExpectedToolsFrom,
            RecipeError::ToolsFromRepeated,
        ];
        for variant in variants {
            let text = variant.to_string();
            assert!(!text.is_empty(), "{variant:?} has no wording");
            for held in held_text(&format!("{variant:?}")) {
                assert!(
                    text.contains(held),
                    "{variant:?} words {text:?} without `{held}`"
                );
            }
        }
    }

    /// The names of the `String` fields in a variant's debug text, which its
    /// test gives each as its own value.
    fn held_text(debug: &str) -> Vec<&str> {
        debug.split('"').skip(1).step_by(2).collect()
    }
}
