//! The errors of `source`, `use`, `path`, `dimensions`, `type` and `exclude` lines and a call's selectors; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `declarations`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeclarationError {
    /// Reads: expected `:` after path product name
    ExpectedColonAfterPathProduct,
    /// Reads: path template must not be empty
    EmptyPathTemplate,
    /// Reads: a default path has one rule; give several rules to a source, as `path <source>:` with a rule on each line beneath it
    DefaultPathHasSeveralRules,
    /// Reads: write a source's rules either after its `:` or on the lines beneath it, not both
    PathRulesAfterAndBeneath,
    /// Reads: import needs a file path
    ImportWithoutFile,
    /// Reads: unterminated quoted import path
    UnterminatedQuotedImportPath,
    /// Reads: expected `as alias` after import path
    ExpectedAliasAfterImportPath,
    /// Reads: expected `dimensions \[first, second, ...\]`
    ExpectedDimensionsList,
    /// Reads: `dimensions` repeats `{dimension}`
    DimensionsRepeats { dimension: String },
    /// Reads: `dimensions` needs at least one dimension
    DimensionsEmpty,
    /// Reads: a source written `beside` another inherits its dimensions; remove its \[dimensions\]
    BesideSourceHasDimensions,
    /// Reads: expected source name, optional `: Type`, optional `.ext`, optional `/` for a folder, and optional \[dimensions\]
    ExpectedSourceForm,
    /// Reads: expected closing `\]` in product declaration
    UnclosedDimensions,
    /// Reads: `{name}` has no dimensions, so it takes no brackets; remove `\[\]`
    EmptyBrackets { name: String },
    /// Reads: a source written `beside` another inherits its dimensions and names only a suffix, as in `source meta : Json .json beside image`
    BesideSourceHasOtherParts,
    /// Reads: `@{keyword}()` needs a dimension
    SelectorNeedsDimension { keyword: String },
    /// `@first` and `@last` each take one dimension.
    SelectionNeedsOneDimension { keyword: String },
    /// An input already selects its first or last artifact.
    SelectionRepeated { previous: String, keyword: String },
    /// Reads: write the dimensions in one clause: `@vary(model, config)`
    VaryInOneClause,
    /// Reads: `@vary(...)` repeats `{dimension}`
    VaryRepeats { dimension: String },
    /// Reads: expected `dimension=value` in `@where(...)`
    ExpectedDimensionValueWhere,
    /// Reads: a `@where` value must be one nonempty token
    WhereValueNotOneToken,
    /// Reads: `@where(...)` pins `{dimension}` twice
    WherePinsTwice { dimension: String },
    /// Reads: `@each(...)` names `{dimension}` twice
    EachRepeats { dimension: String },
    /// Reads: `exclude from` is not a recipe rule; to leave out what a person judged, declare a `review` in the pipeline, attach it with `@review(name)`, and write `review name from file.csv` in the recipe
    ExcludeFromReplaced,
    /// Reads: `exclude` names a source, values such as `\[sub=02\]`, or both
    ExcludeNamesNothing,
    /// Reads: {detail}, for the path of `exclude path` or `prefer path`
    FoundPathInvalid { detail: String },
    /// Reads: `prefer` names the paths to keep, as `prefer path */* - *`
    PreferNeedsPath,
    /// Reads: expected `use path \[as alias\]` or `use name\[, name\] from path \[as alias\]`
    ExpectedUseForm,
    /// Reads: expected `type Name<Param, ...>`, such as `type Frame<Kind, Space>`
    ExpectedTypeDeclaration,
    /// Reads: `{name}` has no parameters, so it needs no declaration; remove this line
    TypeNeedsNoDeclaration { name: String },
    /// Reads: type `{constructor}` has parameter `{param}` twice
    TypeHasParameterTwice { constructor: String, param: String },
    /// Reads: a `here` source is one file kept beside the pipeline, so it takes no \[dimensions\], `/` or `beside`, as in `source plotter : Script .py here`
    HereSourceHasExtras,
}

impl fmt::Display for DeclarationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedColonAfterPathProduct => f.write_str("expected `:` after path product name"),
            Self::EmptyPathTemplate => f.write_str("path template must not be empty"),
            Self::DefaultPathHasSeveralRules => f.write_str("a default path has one rule; give several rules to a source, as `path <source>:` with a rule on each line beneath it"),
            Self::PathRulesAfterAndBeneath => f.write_str("write a source's rules either after its `:` or on the lines beneath it, not both"),
            Self::ImportWithoutFile => f.write_str("import needs a file path"),
            Self::UnterminatedQuotedImportPath => f.write_str("unterminated quoted import path"),
            Self::ExpectedAliasAfterImportPath => f.write_str("expected `as alias` after import path"),
            Self::ExpectedDimensionsList => f.write_str("expected `dimensions [first, second, ...]`"),
            Self::DimensionsRepeats { dimension } => write!(f, "`dimensions` repeats `{dimension}`"),
            Self::DimensionsEmpty => f.write_str("`dimensions` needs at least one dimension"),
            Self::BesideSourceHasDimensions => f.write_str("a source written `beside` another inherits its dimensions; remove its [dimensions]"),
            Self::ExpectedSourceForm => f.write_str("expected source name, optional `: Type`, optional `.ext`, optional `/` for a folder, and optional [dimensions]"),
            Self::UnclosedDimensions => f.write_str("expected closing `]` in product declaration"),
            Self::EmptyBrackets { name } => write!(f, "`{name}` has no dimensions, so it takes no brackets; remove `[]`"),
            Self::BesideSourceHasOtherParts => f.write_str("a source written `beside` another inherits its dimensions and names only a suffix, as in `source meta : Json .json beside image`"),
            Self::SelectorNeedsDimension { keyword } => write!(f, "`@{keyword}()` needs a dimension"),
            Self::SelectionNeedsOneDimension { keyword } => write!(f, "`@{keyword}(...)` takes one dimension"),
            Self::SelectionRepeated { previous, keyword } => write!(f, "`@{keyword}(...)` follows `@{previous}(...)`; give `@first(...)` or `@last(...)` once per input"),
            Self::VaryInOneClause => f.write_str("write the dimensions in one clause: `@vary(model, config)`"),
            Self::VaryRepeats { dimension } => write!(f, "`@vary(...)` repeats `{dimension}`"),
            Self::ExpectedDimensionValueWhere => f.write_str("expected `dimension=value` in `@where(...)`"),
            Self::WhereValueNotOneToken => f.write_str("a `@where` value must be one nonempty token"),
            Self::WherePinsTwice { dimension } => write!(f, "`@where(...)` pins `{dimension}` twice"),
            Self::EachRepeats { dimension } => write!(f, "`@each(...)` names `{dimension}` twice"),
            Self::ExcludeFromReplaced => f.write_str("`exclude from` is not a recipe rule; to leave out what a person judged, declare a `review` in the pipeline, attach it with `@review(name)`, and write `review name from file.csv` in the recipe"),
            Self::ExcludeNamesNothing => f.write_str("`exclude` names a source, values such as `[sub=02]`, or both"),
            Self::FoundPathInvalid { detail } => f.write_str(detail),
            Self::PreferNeedsPath => f.write_str("`prefer` names the paths to keep, as `prefer path */* - *`"),
            Self::ExpectedUseForm => f.write_str("expected `use path [as alias]` or `use name[, name] from path [as alias]`"),
            Self::ExpectedTypeDeclaration => f.write_str("expected `type Name<Param, ...>`, such as `type Frame<Kind, Space>`"),
            Self::TypeNeedsNoDeclaration { name } => write!(f, "`{name}` has no parameters, so it needs no declaration; remove this line"),
            Self::TypeHasParameterTwice { constructor, param } => write!(f, "type `{constructor}` has parameter `{param}` twice"),
            Self::HereSourceHasExtras => f.write_str("a `here` source is one file kept beside the pipeline, so it takes no [dimensions], `/` or `beside`, as in `source plotter : Script .py here`"),
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
            DeclarationError::ExpectedColonAfterPathProduct,
            DeclarationError::EmptyPathTemplate,
            DeclarationError::DefaultPathHasSeveralRules,
            DeclarationError::PathRulesAfterAndBeneath,
            DeclarationError::ImportWithoutFile,
            DeclarationError::UnterminatedQuotedImportPath,
            DeclarationError::ExpectedAliasAfterImportPath,
            DeclarationError::ExpectedDimensionsList,
            DeclarationError::DimensionsRepeats {
                dimension: "dimension".to_owned(),
            },
            DeclarationError::DimensionsEmpty,
            DeclarationError::BesideSourceHasDimensions,
            DeclarationError::ExpectedSourceForm,
            DeclarationError::UnclosedDimensions,
            DeclarationError::EmptyBrackets {
                name: "name".to_owned(),
            },
            DeclarationError::BesideSourceHasOtherParts,
            DeclarationError::SelectorNeedsDimension {
                keyword: "keyword".to_owned(),
            },
            DeclarationError::SelectionNeedsOneDimension {
                keyword: "first".to_owned(),
            },
            DeclarationError::SelectionRepeated {
                previous: "first".to_owned(),
                keyword: "last".to_owned(),
            },
            DeclarationError::VaryInOneClause,
            DeclarationError::VaryRepeats {
                dimension: "dimension".to_owned(),
            },
            DeclarationError::ExpectedDimensionValueWhere,
            DeclarationError::WhereValueNotOneToken,
            DeclarationError::WherePinsTwice {
                dimension: "dimension".to_owned(),
            },
            DeclarationError::EachRepeats {
                dimension: "dimension".to_owned(),
            },
            DeclarationError::ExcludeFromReplaced,
            DeclarationError::ExcludeNamesNothing,
            DeclarationError::FoundPathInvalid {
                detail: "detail".to_owned(),
            },
            DeclarationError::PreferNeedsPath,
            DeclarationError::ExpectedUseForm,
            DeclarationError::ExpectedTypeDeclaration,
            DeclarationError::TypeNeedsNoDeclaration {
                name: "name".to_owned(),
            },
            DeclarationError::TypeHasParameterTwice {
                constructor: "constructor".to_owned(),
                param: "param".to_owned(),
            },
            DeclarationError::HereSourceHasExtras,
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
