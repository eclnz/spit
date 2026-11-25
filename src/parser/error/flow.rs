use std::fmt;

use super::super::flow::{statement_words, PIPELINE};
use crate::parser::File;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlowError {
    LineIndentedDifferently {
        what: String,
    },
    ExpectedAfterDefaultPath,
    ExpectedColonAfterDefaultCheckExtension,
    DefaultPathNamesProduct,
    StageHeaderIndented,
    OpensNoSection {
        line: String,
        instead: String,
    },
    BelongsTopLevelOutsideStage {
        what: String,
        name: String,
    },
    ExpectedRootPath,
    ExpectedOutputPath,
    SidecarsReplaced,
    ExpectedTypesMode,
    DefaultInsideDefault {
        setting: String,
    },
    BelongsSpitinRecipe {
        word: String,
    },
    ExpectedBeforeOperationCall,
    ExpectedStatement,
    ExpectedStep,
    ExpectedOutputProductBefore,
    ClauseOnStepProduct,
    UnknownDefault,
    ExpectedStageHeader,
    ShellSourceReplaced,
    UnknownKeyword {
        word: String,
        suggestion: Option<String>,
    },
    ToolsFromInPipeline,
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineIndentedDifferently { what } => write!(f, "this line is indented differently from the other lines of {what}"),
            Self::ExpectedAfterDefaultPath => f.write_str("expected `:` after `default path`"),
            Self::ExpectedColonAfterDefaultCheckExtension => f.write_str("expected `:` after the extension in `default check .ext:`"),
            Self::DefaultPathNamesProduct => f.write_str("a default path rule names no product; write one product's rule as `path product: ...`, without `default`"),
            Self::StageHeaderIndented => f.write_str("a stage header outside every stage starts at the beginning of its line"),
            Self::OpensNoSection { line, instead } => write!(f, "`{line}` opens no section; {instead}"),
            Self::BelongsTopLevelOutsideStage { what, name } => write!(f, "{what} belongs at the top level, outside stage `{name}`"),
            Self::ExpectedRootPath => f.write_str("expected `root <directory>`, such as `root data`"),
            Self::ExpectedOutputPath => f.write_str("expected `output <directory>`, such as `output results`"),
            Self::SidecarsReplaced => f.write_str("`sidecars` starts no statement; declare each companion as `source name : Type .ext beside sibling`, and give the sibling source the path and dimensions"),
            Self::ExpectedTypesMode => f.write_str("expected `types strict` or `types standard`"),
            Self::DefaultInsideDefault { setting } => write!(f, "a default is written after `default`, as `default {setting}: ...`, or in a `default:` block"),
            Self::BelongsSpitinRecipe { word } => write!(f, "`{word}` belongs in a .spitin recipe, which names its pipeline and the dataset folder it is bound to; a pipeline given alone names its folder with `root` or takes it from `--root`"),
            Self::ExpectedBeforeOperationCall => f.write_str("expected `=` before operation call"),
            Self::ExpectedStatement => f.write_str("expected source, operation, command, verify, resources, check, review, require, path, default, stage, or output = operation(inputs)"),
            Self::ExpectedStep => f.write_str("expected flow step: output = operation(inputs)"),
            Self::ExpectedOutputProductBefore => f.write_str("expected an output product before `=`"),
            Self::ClauseOnStepProduct => f.write_str("a step's product takes no `@check(...)` or `@review(...)`; attach it to the operation's output, as in `-> Image @check(nonempty)`"),
            Self::UnknownDefault => f.write_str("a default is a `path:` rule, an `ext:` extension, a `check:` list, a `review:` list, a `uses:` list of tools or an `entities:` spelling of `{@entities}`, as in `default path: out/{@product}/{@entities}`"),
            Self::ExpectedStageHeader => f.write_str("expected `stage name:`, with the stage's lines indented beneath it"),
            Self::ShellSourceReplaced => f.write_str("`shell-source` starts no statement: a command runs a program, not a shell function, so make the program executable and put it on PATH"),
            Self::UnknownKeyword { word, suggestion } => {
                write!(f, "`{word}` does not start a statement; ")?;
                if let Some(suggestion) = suggestion {
                    write!(f, "did you mean `{suggestion}`? ")?;
                }
                write!(
                    f,
                    "a pipeline line starts with {}, or is a step `output = operation(inputs)`, and a recipe line starts with {PIPELINE}, inputs, {}",
                    statement_words(File::Pipeline),
                    statement_words(File::Recipe),
                )
            },
            Self::ToolsFromInPipeline => f.write_str("`tools from` belongs in a .spitin recipe, which names the `.spitenv` that says what each tool is on the machine the jobs run on"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            FlowError::LineIndentedDifferently {
                what: "what".to_owned(),
            },
            FlowError::ExpectedAfterDefaultPath,
            FlowError::ExpectedColonAfterDefaultCheckExtension,
            FlowError::DefaultPathNamesProduct,
            FlowError::StageHeaderIndented,
            FlowError::OpensNoSection {
                line: "line".to_owned(),
                instead: "instead".to_owned(),
            },
            FlowError::BelongsTopLevelOutsideStage {
                what: "what".to_owned(),
                name: "name".to_owned(),
            },
            FlowError::ExpectedRootPath,
            FlowError::ExpectedOutputPath,
            FlowError::SidecarsReplaced,
            FlowError::ExpectedTypesMode,
            FlowError::DefaultInsideDefault {
                setting: "setting".to_owned(),
            },
            FlowError::BelongsSpitinRecipe {
                word: "word".to_owned(),
            },
            FlowError::ExpectedBeforeOperationCall,
            FlowError::ExpectedStatement,
            FlowError::ExpectedStep,
            FlowError::ExpectedOutputProductBefore,
            FlowError::ClauseOnStepProduct,
            FlowError::UnknownDefault,
            FlowError::ExpectedStageHeader,
            FlowError::ShellSourceReplaced,
            FlowError::UnknownKeyword {
                word: "word".to_owned(),
                suggestion: Some("suggestion".to_owned()),
            },
            FlowError::ToolsFromInPipeline,
        ];
        crate::parser::error::assert_worded(variants);
    }
}
