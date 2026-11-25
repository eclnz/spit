use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LowerError {
    DuplicatePathTemplateProduct {
        product: String,
    },
    DuplicateDefaultPathTemplateStage {
        stage: String,
    },
    DuplicateDefaultPathTemplate,
    DuplicateDefaultExt {
        stage: Option<String>,
    },
    DuplicateDefaultChecks {
        stage: Option<String>,
        extension: Option<String>,
    },
    RulesBelongInRecipe,
    ReviewFileLineInPipeline,
    StatementRepeated {
        what: String,
    },
    PipelineHasDimensionsLine,
    TypeDeclaredWithOtherParameters {
        name: String,
    },
    PipelineHasTypesLine,
    DuplicateEntityLabelDimension {
        dimension: String,
    },
    EntityLabelNamesUnknownDimension {
        dimension: String,
    },
    EntityLabelRepeated {
        label: String,
    },
    CallArgumentCount {
        name: String,
        takes: usize,
        what: String,
        given: usize,
    },
    CallPrefixTaken {
        prefix: String,
        taken: String,
    },
    DuplicateDefaultReviews {
        stage: Option<String>,
    },
    ImportWithoutPath,
    ImportedFileHasDuplicateDefinition {
        name: String,
    },
    ImportNameMatchesBothSourceOperation {
        name: String,
    },
    ImportedFileHasDuplicateCommand {
        name: String,
    },
    DuplicateImport {
        name: String,
    },
    ReviewFileWithoutReview {
        name: String,
    },
    CommandOnOperationWithSteps {
        operation: String,
        keyword: String,
    },
    PathRuleOnBesideSource {
        name: String,
        sibling: String,
    },
    BesideUnknownSource {
        name: String,
        sibling: String,
    },
    BesideWrongKind {
        name: String,
        what: String,
        sibling: String,
    },
    DuplicateDefaultEntities {
        stage: Option<String>,
    },
    EntityLabelWithoutSpelling {
        stage: Option<String>,
    },
    ProductMadeInsideCall {
        product: String,
        outputs: String,
        operation: String,
        line: usize,
    },
    ReviewFileRepeated {
        review: String,
        file: String,
        line: usize,
    },
    ImportHasNoDefinitions {
        path: String,
    },
    ImportBesideSourceAlone {
        name: String,
        group: String,
    },
    ImportNameNotFound {
        name: String,
        path: String,
    },
    CannotLoadImport {
        imported_path: String,
        error: String,
    },
    ImportCycleThrough {
        canonical: String,
    },
    ImportNotRegularFile {
        canonical: String,
    },
    CannotReadImport {
        canonical: String,
        error: String,
    },
    ImportConflict {
        noun: String,
        name: String,
    },
    WhereConflictsWithBody {
        product: String,
        dimension: String,
        previous: String,
        value: String,
    },
    SameConflictsWithBody {
        product: String,
    },
    SelectionConflictsWithBody {
        product: String,
        given: String,
        used: String,
    },
    BodyOutputsUnnamed {
        name: String,
    },
    BodyOutputDeclaresPlace {
        port: String,
        name: String,
    },
    BodyHasMinimum {
        name: String,
    },
    BodyCallsUndeclared {
        called: String,
        name: String,
    },
    BodyReadsUnknown {
        name: String,
        product: String,
    },
    BodyRepeatsProduct {
        name: String,
        output: String,
    },
    BodyOutputUnmade {
        name: String,
        output: String,
    },
    DuplicateOperation {
        name: String,
        line: usize,
        imported: bool,
        global: bool,
    },
    ToolDeclaredTwice {
        name: String,
    },
    DuplicateUses {
        operation: String,
    },
    DuplicateDefaultUses {
        stage: Option<String>,
    },
    AnchoredRepeated {
        name: String,
    },
    ToolNotDeclared {
        name: String,
    },
    UsesOnImportedOperation {
        name: String,
    },
    UsesUndeclaredOperation {
        name: String,
    },
    AnchoredOnImportedOperation {
        name: String,
    },
    AnchoredUndeclaredOperation {
        name: String,
    },
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
            Self::SelectionConflictsWithBody { product, given, used } => write!(f, "`{product}` is given `{given}` here, but the body reads it with `{used}`; choose one selector"),
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
            LowerError::SelectionConflictsWithBody {
                product: "product".to_owned(),
                given: "@first(run)".to_owned(),
                used: "@last(run)".to_owned(),
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
        crate::parser::error::assert_worded(variants);
    }
}
