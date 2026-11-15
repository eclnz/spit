//! Errors the parser reads from `declarations`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `declarations`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeclarationError {
    /// Says: expected `:` after path product name
    ExpectedAfterPathProductName,
    /// Says: path template must not be empty
    PathTemplateMustNotEmpty,
    /// Says: import needs a file path
    ImportNeedsFilePath,
    /// Says: unterminated quoted import path
    UnterminatedQuotedImportPath,
    /// Says: expected `as alias` after import path
    ExpectedAliasAfterImportPath,
    /// Says: expected `dimensions \[first, second, ...\]`
    ExpectedDimensionsFirstSecond,
    /// Says: `dimensions` repeats `{dimension}`
    DimensionsRepeats { dimension: String },
    /// Says: `dimensions` needs at least one dimension
    DimensionsNeedsLeastDimension,
    /// Says: a source written `beside` another inherits its dimensions; remove its \[dimensions\]
    SourceWrittenBesideAnotherInheritsDimensions,
    /// Says: expected source name, optional `: Type`, optional `.ext`, optional `/` for a folder, and optional \[dimensions\]
    ExpectedSourceNameOptionalTypeOptional,
    /// Says: expected closing `\]` in product declaration
    ExpectedClosingProductDeclaration,
    /// Says: `{name}` has no dimensions, so it takes no brackets; remove `\[\]`
    HasNoDimensionsSoTakesNo { name: String },
    /// Says: a source written `beside` another inherits its dimensions and names only a suffix, as in `source meta : Json .json beside image`
    SourceWrittenBesideAnotherInheritsDimensions2,
    /// Says: `@{keyword}()` needs a dimension
    NeedsDimension { keyword: String },
    /// Says: duplicate `@{keyword}(...)`
    Duplicate { keyword: String },
    /// Says: write the dimensions in one clause: `@vary(model, config)`
    WriteDimensionsClauseVaryModelConfig,
    /// Says: `@vary(...)` repeats `{dimension}`
    VaryRepeats { dimension: String },
    /// Says: expected `dimension=value` in `@where(...)`
    ExpectedDimensionValueWhere,
    /// Says: a `@where` value must be one nonempty token
    WhereValueMustNonemptyToken,
    /// Says: `@where(...)` pins `{dimension}` twice
    WherePinsTwice { dimension: String },
    /// Says: `@each(...)` names `{dimension}` twice
    NamesTwice { dimension: String },
    /// Says: `exclude from` is not a recipe rule; to leave out what a person judged, declare a `review` in the pipeline, attach it with `@review(name)`, and write `review name from file.csv` in the recipe
    ExcludeNotRecipeRuleLeaveOut,
    /// Says: `exclude` names a source, values such as `\[sub=02\]`, or both
    ExcludeNamesSourceValuesSuchSub,
    /// Says: expected `use path \[as alias\]` or `use name\[, name\] from path \[as alias\]`
    ExpectedUsePathAliasUseName,
    /// Says: expected `type Name<Param, ...>`, such as `type Frame<Kind, Space>`
    ExpectedTypeNameParamSuchType,
    /// Says: `{name}` has no parameters, so it needs no declaration; remove this line
    HasNoParametersSoNeedsNo { name: String },
    /// Says: type `{constructor}` has parameter `{param}` twice
    TypeHasParameterTwice { constructor: String, param: String },
}

impl fmt::Display for DeclarationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedAfterPathProductName => f.write_str("expected `:` after path product name"),
            Self::PathTemplateMustNotEmpty => f.write_str("path template must not be empty"),
            Self::ImportNeedsFilePath => f.write_str("import needs a file path"),
            Self::UnterminatedQuotedImportPath => f.write_str("unterminated quoted import path"),
            Self::ExpectedAliasAfterImportPath => f.write_str("expected `as alias` after import path"),
            Self::ExpectedDimensionsFirstSecond => f.write_str("expected `dimensions [first, second, ...]`"),
            Self::DimensionsRepeats { dimension } => write!(f, "`dimensions` repeats `{dimension}`"),
            Self::DimensionsNeedsLeastDimension => f.write_str("`dimensions` needs at least one dimension"),
            Self::SourceWrittenBesideAnotherInheritsDimensions => f.write_str("a source written `beside` another inherits its dimensions; remove its [dimensions]"),
            Self::ExpectedSourceNameOptionalTypeOptional => f.write_str("expected source name, optional `: Type`, optional `.ext`, optional `/` for a folder, and optional [dimensions]"),
            Self::ExpectedClosingProductDeclaration => f.write_str("expected closing `]` in product declaration"),
            Self::HasNoDimensionsSoTakesNo { name } => write!(f, "`{name}` has no dimensions, so it takes no brackets; remove `[]`"),
            Self::SourceWrittenBesideAnotherInheritsDimensions2 => f.write_str("a source written `beside` another inherits its dimensions and names only a suffix, as in `source meta : Json .json beside image`"),
            Self::NeedsDimension { keyword } => write!(f, "`@{keyword}()` needs a dimension"),
            Self::Duplicate { keyword } => write!(f, "duplicate `@{keyword}(...)`"),
            Self::WriteDimensionsClauseVaryModelConfig => f.write_str("write the dimensions in one clause: `@vary(model, config)`"),
            Self::VaryRepeats { dimension } => write!(f, "`@vary(...)` repeats `{dimension}`"),
            Self::ExpectedDimensionValueWhere => f.write_str("expected `dimension=value` in `@where(...)`"),
            Self::WhereValueMustNonemptyToken => f.write_str("a `@where` value must be one nonempty token"),
            Self::WherePinsTwice { dimension } => write!(f, "`@where(...)` pins `{dimension}` twice"),
            Self::NamesTwice { dimension } => write!(f, "`@each(...)` names `{dimension}` twice"),
            Self::ExcludeNotRecipeRuleLeaveOut => f.write_str("`exclude from` is not a recipe rule; to leave out what a person judged, declare a `review` in the pipeline, attach it with `@review(name)`, and write `review name from file.csv` in the recipe"),
            Self::ExcludeNamesSourceValuesSuchSub => f.write_str("`exclude` names a source, values such as `[sub=02]`, or both"),
            Self::ExpectedUsePathAliasUseName => f.write_str("expected `use path [as alias]` or `use name[, name] from path [as alias]`"),
            Self::ExpectedTypeNameParamSuchType => f.write_str("expected `type Name<Param, ...>`, such as `type Frame<Kind, Space>`"),
            Self::HasNoParametersSoNeedsNo { name } => write!(f, "`{name}` has no parameters, so it needs no declaration; remove this line"),
            Self::TypeHasParameterTwice { constructor, param } => write!(f, "type `{constructor}` has parameter `{param}` twice"),
        }
    }
}
