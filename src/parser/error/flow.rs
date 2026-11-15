//! Errors the parser reads from `flow`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `flow`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlowError {
    /// Says: this line is indented differently from the other lines of {what}
    LineIndentedDifferently { what: String },
    /// Says: expected `:` after `default path`
    ExpectedAfterDefaultPath,
    /// Says: a default path rule names no product; write one product's rule as `path product: ...`, without `default`
    DefaultPathNamesProduct,
    /// Says: a stage header outside every stage starts at the beginning of its line
    StageHeaderIndented,
    /// Says: `{line}` opens no section; {instead}
    OpensNoSection { line: String, instead: String },
    /// Says: {what} belongs at the top level, outside stage `{name}`
    BelongsTopLevelOutsideStage { what: String, name: String },
    /// Says: expected `root <directory>`, such as `root data`
    ExpectedRootPath,
    /// Says: expected `output <directory>`, such as `output results`
    ExpectedOutputPath,
    /// Says: `sidecars` starts no statement; declare each companion as `source name : Type .ext beside sibling`, and give the sibling source the path and dimensions
    SidecarsReplaced,
    /// Says: expected `types strict` or `types standard`
    ExpectedTypesMode,
    /// Says: a default is written after `default`, as `default {setting}: ...`, or in a `default:` block
    DefaultInsideDefault { setting: String },
    /// Says: `{word}` belongs in a .spitin recipe, which names its pipeline and the dataset folder it is bound to; a pipeline given alone names its folder with `root` or takes it from `--root`
    BelongsSpitinRecipe { word: String },
    /// Says: expected `=` before operation call
    ExpectedBeforeOperationCall,
    /// Says: expected source, operation, command, verify, resources, check, review, require, path, default, stage, or output = operation(inputs)
    ExpectedStatement,
    /// Says: expected flow step: output = operation(inputs)
    ExpectedStep,
    /// Says: expected an output product before `=`
    ExpectedOutputProductBefore,
    /// Says: a step's product takes no `@check(...)` or `@review(...)`; attach it to the operation's output, as in `-> Image @check(nonempty)`
    StepSProductTakesNoCheck,
    /// Says: a default is a `path:` rule, an `ext:` extension, a `check:` list, a `review:` list or an `entities:` spelling of `{@entities}`, as in `default path: out/{@product}/{@entities}`
    UnknownDefault,
    /// Says: expected `stage name:`, with the stage's lines indented beneath it
    ExpectedStageHeader,
    /// Says: `shell-source` starts no statement: a command runs a program, not a shell function, so make the program executable and put it on PATH
    ShellSourceReplaced,
    /// Says: `{word}` does not start a statement; {hint}a pipeline line starts with                  {pipeline_words}, or is a step `output = operation(inputs)`, and a recipe line starts                  with {pipeline}, inputs, {recipe_words}
    UnknownKeyword {
        word: String,
        hint: String,
        pipeline_words: String,
        pipeline: String,
        recipe_words: String,
    },
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineIndentedDifferently { what } => write!(f, "this line is indented differently from the other lines of {what}"),
            Self::ExpectedAfterDefaultPath => f.write_str("expected `:` after `default path`"),
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
            Self::StepSProductTakesNoCheck => f.write_str("a step's product takes no `@check(...)` or `@review(...)`; attach it to the operation's output, as in `-> Image @check(nonempty)`"),
            Self::UnknownDefault => f.write_str("a default is a `path:` rule, an `ext:` extension, a `check:` list, a `review:` list or an `entities:` spelling of `{@entities}`, as in `default path: out/{@product}/{@entities}`"),
            Self::ExpectedStageHeader => f.write_str("expected `stage name:`, with the stage's lines indented beneath it"),
            Self::ShellSourceReplaced => f.write_str("`shell-source` starts no statement: a command runs a program, not a shell function, so make the program executable and put it on PATH"),
            Self::UnknownKeyword { word, hint, pipeline_words, pipeline, recipe_words } => write!(f, "`{word}` does not start a statement; {hint}a pipeline line starts with \
                 {pipeline_words}, or is a step `output = operation(inputs)`, and a recipe line starts \
                 with {pipeline}, inputs, {recipe_words}"),
        }
    }
}
