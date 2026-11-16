//! The errors of lowering a document: duplicates, imports, calls and bodies; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `lower`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LowerError {
    /// Reads: duplicate path template for product `{product}`
    DuplicatePathTemplateProduct { product: String },
    /// Reads: duplicate default path template for stage `{stage}`
    DuplicateDefaultPathTemplateStage { stage: String },
    /// Reads: duplicate default path template
    DuplicateDefaultPathTemplate,
    /// A `default ext:` written twice for the pipeline or one stage.
    DuplicateDefaultExt { stage: Option<String> },
    /// A `default check:` list written twice for the pipeline or one stage, or a `default check .ext:` list written twice for one extension.
    DuplicateDefaultChecks {
        stage: Option<String>,
        extension: Option<String>,
    },
    /// Reads: `require` and `exclude` rules belong in a .spitin recipe, not a pipeline
    RulesBelongInRecipe,
    /// Reads: `review ... from` lines belong in a .spitin recipe; a pipeline declares a review as `review name`
    ReviewFileLineInPipeline,
    /// Reads: a .spit names its {what} once
    StatementRepeated { what: String },
    /// Reads: a pipeline has one `dimensions` line
    PipelineHasDimensionsLine,
    /// Reads: type `{name}` is already declared with other parameters
    TypeDeclaredWithOtherParameters { name: String },
    /// Reads: a pipeline has one `types` line
    PipelineHasTypesLine,
    /// Reads: duplicate entity label for dimension `{dimension}`
    DuplicateEntityLabelDimension { dimension: String },
    /// Reads: entity label names unknown dimension `{dimension}`
    EntityLabelNamesUnknownDimension { dimension: String },
    /// Reads: entity label `{label}` is used for more than one dimension
    EntityLabelRepeated { label: String },
    /// Reads: operation `{name}` takes {takes} {what}, but this call gives {given}
    CallArgumentCount {
        name: String,
        takes: usize,
        what: String,
        given: usize,
    },
    /// Reads: this call files the products it makes for itself under `{prefix}`, as an import does `{taken}`; rename the call's first output or the import's alias
    CallPrefixTaken { prefix: String, taken: String },
    /// A `default review:` list written twice for the pipeline or one stage.
    DuplicateDefaultReviews { stage: Option<String> },
    /// Reads: imports require a document path; use parse_pipeline_at
    ImportWithoutPath,
    /// Reads: imported file has duplicate definition `{name}`
    ImportedFileHasDuplicateDefinition { name: String },
    /// Reads: import name `{name}` matches both a source and an operation
    ImportNameMatchesBothSourceOperation { name: String },
    /// Reads: imported file has duplicate command for `{name}`
    ImportedFileHasDuplicateCommand { name: String },
    /// Reads: duplicate import `{name}`
    DuplicateImport { name: String },
    /// Reads: a recipe names the file of a review the pipeline declares, as `review {name} from file.csv`; declare it in the pipeline with `review {name}`
    ReviewFileWithoutReview { name: String },
    /// Reads: operation `{operation}` is carried out by the steps in its body, so it takes no `{keyword}` line; give each step's operation its own
    CommandOnOperationWithSteps { operation: String, keyword: String },
    /// Reads: source `{name}` is beside `{sibling}`, so its path follows that source; remove its path rule
    PathRuleOnBesideSource { name: String, sibling: String },
    /// Reads: source `{name}` is beside unknown source `{sibling}`; declare `{sibling}` first
    BesideUnknownSource { name: String, sibling: String },
    /// Reads: source `{name}` must be beside {what} not `{sibling}`
    BesideWrongKind {
        name: String,
        what: String,
        sibling: String,
    },
    /// A `default entities:` spelling written twice for the pipeline or one stage.
    DuplicateDefaultEntities { stage: Option<String> },
    /// An entity label that renames a dimension where no `default entities:` spelling exists.
    EntityLabelWithoutSpelling { stage: Option<String> },
    /// Reads: `{product}` is made inside the call `{outputs} = {operation}(...)` on line {line}; make it an output of `{operation}` to read it here
    ProductMadeInsideCall {
        product: String,
        outputs: String,
        operation: String,
        line: usize,
    },
    /// Reads: review `{review}` already has its file `{file}` on line {line}; a review names one file
    ReviewFileRepeated {
        review: String,
        file: String,
        line: usize,
    },
    /// Reads: `{path}` contains no reusable definitions
    ImportHasNoDefinitions { path: String },
    /// Reads: source `{name}` is beside `{group}`; import `{group}` to bring its companions
    ImportBesideSourceAlone { name: String, group: String },
    /// Reads: `{name}` is not a source, operation, check or review in `{path}`
    ImportNameNotFound { name: String, path: String },
    /// Reads: cannot load import `{imported_path}`: {error}
    CannotLoadImport {
        imported_path: String,
        error: String,
    },
    /// Reads: import cycle through `{canonical}`
    ImportCycleThrough { canonical: String },
    /// Reads: import `{canonical}` is not a regular file
    ImportNotRegularFile { canonical: String },
    /// Reads: cannot read import `{canonical}`: {error}
    CannotReadImport { canonical: String, error: String },
    /// Reads: import conflicts with {noun} `{name}`
    ImportConflict { noun: String, name: String },
    /// Reads: `{product}` is given `@where({dimension}={previous})` here, but the body reads it with `@where({dimension}={value})`
    WhereConflictsWithBody {
        product: String,
        dimension: String,
        previous: String,
        value: String,
    },
    /// Reads: `{product}` is given `@same(...)` here, and the body reads it with another; give it once
    SameConflictsWithBody { product: String },
    /// Reads: operation `{name}` has a body, so it names each output its steps assign, as in `-> (result: Type)`
    BodyOutputsUnnamed { name: String },
    /// Reads: output `{port}` of `{name}` takes its extension, folder and place from the step that writes it; write only its name and type
    BodyOutputDeclaresPlace { port: String, name: String },
    /// Reads: operation `{name}` has a body; write `@min(...)` on the `many` input of the step that collects
    BodyHasMinimum { name: String },
    /// Reads: operation `{called}` must be declared before `{name}`, whose body calls it
    BodyCallsUndeclared { called: String, name: String },
    /// Reads: the body of `{name}` reads `{product}`, which is neither one of its inputs nor made by an earlier step of it
    BodyReadsUnknown { name: String, product: String },
    /// Reads: the body of `{name}` already has `{output}`; name each product once
    BodyRepeatsProduct { name: String, output: String },
    /// Reads: no step in the body of `{name}` makes its output `{output}`
    BodyOutputUnmade { name: String, output: String },
    /// An operation declared twice, or declared where an import already brought one.
    DuplicateOperation {
        name: String,
        line: usize,
        imported: bool,
        global: bool,
    },
    /// Reads: tool `{name}` is already declared
    ToolDeclaredTwice { name: String },
    /// Reads: duplicate `uses` line for operation `{operation}`; write its tools in one line
    DuplicateUses { operation: String },
    /// A `default uses:` list written twice for the pipeline or one stage.
    DuplicateDefaultUses { stage: Option<String> },
    /// Reads: operation `{name}` is already marked `anchored`
    AnchoredRepeated { name: String },
    /// Reads: tool `{name}` is not declared; add `tool {name}`, and say what it is in the recipe's `.spitenv`
    ToolNotDeclared { name: String },
    /// Reads: operation `{name}` is imported; its tools are set by `uses` in the file that declares it
    UsesOnImportedOperation { name: String },
    /// Reads: `uses` names operation `{name}`, which is not declared
    UsesUndeclaredOperation { name: String },
    /// Reads: operation `{name}` is imported; mark it `anchored` in the file that declares it
    AnchoredOnImportedOperation { name: String },
    /// Reads: `anchored` names operation `{name}`, which is not declared
    AnchoredUndeclaredOperation { name: String },
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePathTemplateProduct { product } => write!(f, "duplicate path template for product `{product}`"),
            Self::DuplicateDefaultPathTemplateStage { stage } => write!(f, "duplicate default path template for stage `{stage}`"),
            Self::DuplicateDefaultPathTemplate => f.write_str("duplicate default path template"),
            Self::DuplicateDefaultExt { stage } => write!(f, "duplicate `default ext:`{}", ForStage(stage)),
            Self::DuplicateDefaultChecks { stage, extension: None } => write!(f, "duplicate `default check:` list{}; write the checks in one line", ForStage(stage)),
            Self::DuplicateDefaultChecks { stage, extension: Some(extension) } => write!(f, "duplicate `default check {extension}:` list{}; write the checks in one line", ForStage(stage)),
            Self::RulesBelongInRecipe => f.write_str("`require` and `exclude` rules belong in a .spitin recipe, not a pipeline"),
            Self::ReviewFileLineInPipeline => f.write_str("`review ... from` lines belong in a .spitin recipe; a pipeline declares a review as `review name`"),
            Self::StatementRepeated { what } => write!(f, "a .spit names its {what} once"),
            Self::PipelineHasDimensionsLine => f.write_str("a pipeline has one `dimensions` line"),
            Self::TypeDeclaredWithOtherParameters { name } => write!(f, "type `{name}` is already declared with other parameters"),
            Self::PipelineHasTypesLine => f.write_str("a pipeline has one `types` line"),
            Self::DuplicateEntityLabelDimension { dimension } => write!(f, "duplicate entity label for dimension `{dimension}`"),
            Self::EntityLabelNamesUnknownDimension { dimension } => write!(f, "entity label names unknown dimension `{dimension}`"),
            Self::EntityLabelRepeated { label } => write!(f, "entity label `{label}` is used for more than one dimension"),
            Self::CallArgumentCount { name, takes, what, given } => write!(f, "operation `{name}` takes {takes} {what}, but this call gives {given}"),
            Self::CallPrefixTaken { prefix, taken } => write!(f, "this call files the products it makes for itself under `{prefix}`, as an import does `{taken}`; rename the call's first output or the import's alias"),
            Self::DuplicateDefaultReviews { stage } => write!(f, "duplicate `default review:` list{}; write the reviews in one line", ForStage(stage)),
            Self::ImportWithoutPath => f.write_str("imports require a document path; use parse_pipeline_at"),
            Self::ImportedFileHasDuplicateDefinition { name } => write!(f, "imported file has duplicate definition `{name}`"),
            Self::ImportNameMatchesBothSourceOperation { name } => write!(f, "import name `{name}` matches both a source and an operation"),
            Self::ImportedFileHasDuplicateCommand { name } => write!(f, "imported file has duplicate command for `{name}`"),
            Self::DuplicateImport { name } => write!(f, "duplicate import `{name}`"),
            Self::ReviewFileWithoutReview { name } => write!(f, "a recipe names the file of a review the pipeline declares, as `review {name} from file.csv`; declare it in the pipeline with `review {name}`"),
            Self::CommandOnOperationWithSteps { operation, keyword } => write!(f, "operation `{operation}` is carried out by the steps in its body, so it takes no `{keyword}` line; give each step's operation its own"),
            Self::PathRuleOnBesideSource { name, sibling } => write!(f, "source `{name}` is beside `{sibling}`, so its path follows that source; remove its path rule"),
            Self::BesideUnknownSource { name, sibling } => write!(f, "source `{name}` is beside unknown source `{sibling}`; declare `{sibling}` first"),
            Self::BesideWrongKind { name, what, sibling } => write!(f, "source `{name}` must be beside {what} not `{sibling}`"),
            Self::DuplicateDefaultEntities { stage } => write!(f, "duplicate `default entities:`{}", ForStage(stage)),
            Self::EntityLabelWithoutSpelling { stage } => write!(f, "an entity label renames a dimension in a `default entities:` spelling, and there is none{}; add one, as `default entities: {{@key}}={{@value}} separated \"__\"`", ForStage(stage)),
            Self::ProductMadeInsideCall { product, outputs, operation, line } => write!(f, "`{product}` is made inside the call `{outputs} = {operation}(...)` on line {line}; make it an output of `{operation}` to read it here"),
            Self::ReviewFileRepeated { review, file, line } => write!(f, "review `{review}` already has its file `{file}` on line {line}; a review names one file"),
            Self::ImportHasNoDefinitions { path } => write!(f, "`{path}` contains no reusable definitions"),
            Self::ImportBesideSourceAlone { name, group } => write!(f, "source `{name}` is beside `{group}`; import `{group}` to bring its companions"),
            Self::ImportNameNotFound { name, path } => write!(f, "`{name}` is not a source, operation, check or review in `{path}`"),
            Self::CannotLoadImport { imported_path, error } => write!(f, "cannot load import `{imported_path}`: {error}"),
            Self::ImportCycleThrough { canonical } => write!(f, "import cycle through `{canonical}`"),
            Self::ImportNotRegularFile { canonical } => write!(f, "import `{canonical}` is not a regular file"),
            Self::CannotReadImport { canonical, error } => write!(f, "cannot read import `{canonical}`: {error}"),
            Self::ImportConflict { noun, name } => write!(f, "import conflicts with {noun} `{name}`"),
            Self::WhereConflictsWithBody { product, dimension, previous, value } => write!(f, "`{product}` is given `@where({dimension}={previous})` here, but the body reads it with `@where({dimension}={value})`"),
            Self::SameConflictsWithBody { product } => write!(f, "`{product}` is given `@same(...)` here, and the body reads it with another; give it once"),
            Self::BodyOutputsUnnamed { name } => write!(f, "operation `{name}` has a body, so it names each output its steps assign, as in `-> (result: Type)`"),
            Self::BodyOutputDeclaresPlace { port, name } => write!(f, "output `{port}` of `{name}` takes its extension, folder and place from the step that writes it; write only its name and type"),
            Self::BodyHasMinimum { name } => write!(f, "operation `{name}` has a body; write `@min(...)` on the `many` input of the step that collects"),
            Self::BodyCallsUndeclared { called, name } => write!(f, "operation `{called}` must be declared before `{name}`, whose body calls it"),
            Self::BodyReadsUnknown { name, product } => write!(f, "the body of `{name}` reads `{product}`, which is neither one of its inputs nor made by an earlier step of it"),
            Self::BodyRepeatsProduct { name, output } => write!(f, "the body of `{name}` already has `{output}`; name each product once"),
            Self::BodyOutputUnmade { name, output } => write!(f, "no step in the body of `{name}` makes its output `{output}`"),
            Self::DuplicateOperation { name, line, imported, global } => {
                write!(f, "duplicate operation `{name}`: ")?;
                if *imported {
                    write!(f, "the `use` on line {line} imports one")?;
                } else {
                    write!(f, "it is already declared on line {line}")?;
                }
                if *global {
                    f.write_str("; operations are global even when declared in a stage, so give this one another name")?;
                }
                Ok(())
            },
            Self::ToolDeclaredTwice { name } => write!(f, "tool `{name}` is already declared"),
            Self::DuplicateUses { operation } => write!(f, "duplicate `uses` line for operation `{operation}`; write its tools in one line"),
            Self::DuplicateDefaultUses { stage } => write!(f, "duplicate `default uses:` list{}; write the tools in one line", ForStage(stage)),
            Self::AnchoredRepeated { name } => write!(f, "operation `{name}` is already marked `anchored`"),
            Self::ToolNotDeclared { name } => write!(f, "tool `{name}` is not declared; add `tool {name}`, and say what it is in the recipe's `.spitenv`"),
            Self::UsesOnImportedOperation { name } => write!(f, "operation `{name}` is imported; its tools are set by `uses` in the file that declares it"),
            Self::UsesUndeclaredOperation { name } => write!(f, "`uses` names operation `{name}`, which is not declared"),
            Self::AnchoredOnImportedOperation { name } => write!(f, "operation `{name}` is imported; mark it `anchored` in the file that declares it"),
            Self::AnchoredUndeclaredOperation { name } => write!(f, "`anchored` names operation `{name}`, which is not declared"),
        }
    }
}

/// ` for stage `name``, or nothing for the pipeline's own default.
struct ForStage<'a>(&'a Option<String>);

impl fmt::Display for ForStage<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(stage) => write!(f, " for stage `{stage}`"),
            None => Ok(()),
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
            LowerError::DuplicatePathTemplateProduct {
                product: "product".to_owned(),
            },
            LowerError::DuplicateDefaultPathTemplateStage {
                stage: "stage".to_owned(),
            },
            LowerError::DuplicateDefaultPathTemplate,
            LowerError::DuplicateDefaultExt {
                stage: Some("stage".to_owned()),
            },
            LowerError::DuplicateDefaultChecks {
                stage: Some("stage".to_owned()),
                extension: Some("extension".to_owned()),
            },
            LowerError::RulesBelongInRecipe,
            LowerError::ReviewFileLineInPipeline,
            LowerError::StatementRepeated {
                what: "what".to_owned(),
            },
            LowerError::PipelineHasDimensionsLine,
            LowerError::TypeDeclaredWithOtherParameters {
                name: "name".to_owned(),
            },
            LowerError::PipelineHasTypesLine,
            LowerError::DuplicateEntityLabelDimension {
                dimension: "dimension".to_owned(),
            },
            LowerError::EntityLabelNamesUnknownDimension {
                dimension: "dimension".to_owned(),
            },
            LowerError::EntityLabelRepeated {
                label: "label".to_owned(),
            },
            LowerError::CallArgumentCount {
                name: "name".to_owned(),
                takes: 1,
                what: "what".to_owned(),
                given: 1,
            },
            LowerError::CallPrefixTaken {
                prefix: "prefix".to_owned(),
                taken: "taken".to_owned(),
            },
            LowerError::DuplicateDefaultReviews {
                stage: Some("stage".to_owned()),
            },
            LowerError::ImportWithoutPath,
            LowerError::ImportedFileHasDuplicateDefinition {
                name: "name".to_owned(),
            },
            LowerError::ImportNameMatchesBothSourceOperation {
                name: "name".to_owned(),
            },
            LowerError::ImportedFileHasDuplicateCommand {
                name: "name".to_owned(),
            },
            LowerError::DuplicateImport {
                name: "name".to_owned(),
            },
            LowerError::ReviewFileWithoutReview {
                name: "name".to_owned(),
            },
            LowerError::CommandOnOperationWithSteps {
                operation: "operation".to_owned(),
                keyword: "keyword".to_owned(),
            },
            LowerError::PathRuleOnBesideSource {
                name: "name".to_owned(),
                sibling: "sibling".to_owned(),
            },
            LowerError::BesideUnknownSource {
                name: "name".to_owned(),
                sibling: "sibling".to_owned(),
            },
            LowerError::BesideWrongKind {
                name: "name".to_owned(),
                what: "what".to_owned(),
                sibling: "sibling".to_owned(),
            },
            LowerError::DuplicateDefaultEntities {
                stage: Some("stage".to_owned()),
            },
            LowerError::EntityLabelWithoutSpelling {
                stage: Some("stage".to_owned()),
            },
            LowerError::ProductMadeInsideCall {
                product: "product".to_owned(),
                outputs: "outputs".to_owned(),
                operation: "operation".to_owned(),
                line: 1,
            },
            LowerError::ReviewFileRepeated {
                review: "review".to_owned(),
                file: "file".to_owned(),
                line: 1,
            },
            LowerError::ImportHasNoDefinitions {
                path: "path".to_owned(),
            },
            LowerError::ImportBesideSourceAlone {
                name: "name".to_owned(),
                group: "group".to_owned(),
            },
            LowerError::ImportNameNotFound {
                name: "name".to_owned(),
                path: "path".to_owned(),
            },
            LowerError::CannotLoadImport {
                imported_path: "imported_path".to_owned(),
                error: "error".to_owned(),
            },
            LowerError::ImportCycleThrough {
                canonical: "canonical".to_owned(),
            },
            LowerError::ImportNotRegularFile {
                canonical: "canonical".to_owned(),
            },
            LowerError::CannotReadImport {
                canonical: "canonical".to_owned(),
                error: "error".to_owned(),
            },
            LowerError::ImportConflict {
                noun: "noun".to_owned(),
                name: "name".to_owned(),
            },
            LowerError::WhereConflictsWithBody {
                product: "product".to_owned(),
                dimension: "dimension".to_owned(),
                previous: "previous".to_owned(),
                value: "value".to_owned(),
            },
            LowerError::SameConflictsWithBody {
                product: "product".to_owned(),
            },
            LowerError::BodyOutputsUnnamed {
                name: "name".to_owned(),
            },
            LowerError::BodyOutputDeclaresPlace {
                port: "port".to_owned(),
                name: "name".to_owned(),
            },
            LowerError::BodyHasMinimum {
                name: "name".to_owned(),
            },
            LowerError::BodyCallsUndeclared {
                called: "called".to_owned(),
                name: "name".to_owned(),
            },
            LowerError::BodyReadsUnknown {
                name: "name".to_owned(),
                product: "product".to_owned(),
            },
            LowerError::BodyRepeatsProduct {
                name: "name".to_owned(),
                output: "output".to_owned(),
            },
            LowerError::BodyOutputUnmade {
                name: "name".to_owned(),
                output: "output".to_owned(),
            },
            LowerError::DuplicateOperation {
                name: "name".to_owned(),
                line: 1,
                imported: false,
                global: false,
            },
            LowerError::ToolDeclaredTwice {
                name: "name".to_owned(),
            },
            LowerError::DuplicateUses {
                operation: "operation".to_owned(),
            },
            LowerError::DuplicateDefaultUses {
                stage: Some("stage".to_owned()),
            },
            LowerError::AnchoredRepeated {
                name: "name".to_owned(),
            },
            LowerError::ToolNotDeclared {
                name: "name".to_owned(),
            },
            LowerError::UsesOnImportedOperation {
                name: "name".to_owned(),
            },
            LowerError::UsesUndeclaredOperation {
                name: "name".to_owned(),
            },
            LowerError::AnchoredOnImportedOperation {
                name: "name".to_owned(),
            },
            LowerError::AnchoredUndeclaredOperation {
                name: "name".to_owned(),
            },
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
