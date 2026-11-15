//! Errors the parser reads from `lower`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `lower`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LowerError {
    /// Says: duplicate path template for product `{product}`
    DuplicatePathTemplateProduct { product: String },
    /// Says: duplicate default path template for stage `{stage}`
    DuplicateDefaultPathTemplateStage { stage: String },
    /// Says: duplicate default path template
    DuplicateDefaultPathTemplate,
    /// Says: duplicate `default ext:`{whose}
    DuplicateDefaultExt { whose: String },
    /// Says: duplicate `default check:` list{whose}; write the checks in one line
    DuplicateDefaultChecks { whose: String },
    /// Says: `require` and `exclude` rules belong in a .spitin recipe, not a pipeline
    RulesBelongInRecipe,
    /// Says: `review ... from` lines belong in a .spitin recipe; a pipeline declares a review as `review name`
    ReviewFileLineInPipeline,
    /// Says: a .spit names its {what} once
    StatementRepeated { what: String },
    /// Says: a pipeline has one `dimensions` line
    PipelineHasDimensionsLine,
    /// Says: type `{name}` is already declared with other parameters
    TypeDeclaredWithOtherParameters { name: String },
    /// Says: a pipeline has one `types` line
    PipelineHasTypesLine,
    /// Says: duplicate entity label for dimension `{dimension}`
    DuplicateEntityLabelDimension { dimension: String },
    /// Says: entity label names unknown dimension `{dimension}`
    EntityLabelNamesUnknownDimension { dimension: String },
    /// Says: entity label `{label}` is used for more than one dimension
    EntityLabelRepeated { label: String },
    /// Says: operation `{name}` takes {takes} {what}, but this call gives {given}
    CallArgumentCount {
        name: String,
        takes: String,
        what: String,
        given: String,
    },
    /// Says: this call files the products it makes for itself under `{prefix}`, as an import does `{taken}`; rename the call's first output or the import's alias
    CallPrefixTaken { prefix: String, taken: String },
    /// Says: duplicate `default review:` list{whose}; write the reviews in one line
    DuplicateDefaultReviews { whose: String },
    /// Says: imports require a document path; use parse_pipeline_at
    ImportWithoutPath,
    /// Says: imported file has duplicate definition `{name}`
    ImportedFileHasDuplicateDefinition { name: String },
    /// Says: import name `{name}` matches both a source and an operation
    ImportNameMatchesBothSourceOperation { name: String },
    /// Says: imported file has duplicate command for `{name}`
    ImportedFileHasDuplicateCommand { name: String },
    /// Says: duplicate import `{name}`
    DuplicateImport { name: String },
    /// Says: a recipe names the file of a review the pipeline declares, as `review {name} from file.csv`; declare it in the pipeline with `review {name}`
    ReviewFileWithoutReview { name: String },
    /// Says: operation `{operation}` is carried out by the steps in its body, so it takes no `{keyword}` line; give each step's operation its own
    CommandOnOperationWithSteps { operation: String, keyword: String },
    /// Says: source `{name}` is beside `{sibling}`, so its path follows that source; remove its path rule
    PathRuleOnBesideSource { name: String, sibling: String },
    /// Says: source `{name}` is beside unknown source `{sibling}`; declare `{sibling}` first
    BesideUnknownSource { name: String, sibling: String },
    /// Says: source `{name}` must be beside {what} not `{sibling}`
    BesideWrongKind {
        name: String,
        what: String,
        sibling: String,
    },
    /// Says: duplicate `default entities:`{scope}
    DuplicateDefaultEntities { scope: String },
    /// Says: an entity label renames a dimension in a `default entities:` spelling, and there is none{scope}; add one, as `default entities: {@key}={@value} separated  "__ "`
    EntityLabelWithoutSpelling { scope: String },
    /// Says: `{product}` is made inside the call `{outputs} = {operation}(...)` on line {line}; make it an output of `{operation}` to read it here
    ProductMadeInsideCall {
        product: String,
        outputs: String,
        operation: String,
        line: String,
    },
    /// Says: review `{review}` already has its file `{file}` on line {line}; a review names one file
    ReviewFileRepeated {
        review: String,
        file: String,
        line: String,
    },
    /// Says: `{path}` contains no reusable definitions
    ImportHasNoDefinitions { path: String },
    /// Says: source `{name}` is beside `{group}`; import `{group}` to bring its companions
    ImportBesideSourceAlone { name: String, group: String },
    /// Says: `{name}` is not a source, operation, check or review in `{path}`
    ImportNameNotFound { name: String, path: String },
    /// Says: cannot load import `{imported_path}`: {error}
    CannotLoadImport {
        imported_path: String,
        error: String,
    },
    /// Says: import cycle through `{canonical}`
    ImportCycleThrough { canonical: String },
    /// Says: import `{canonical}` is not a regular file
    ImportNotRegularFile { canonical: String },
    /// Says: cannot read import `{canonical}`: {error}
    CannotReadImport { canonical: String, error: String },
    /// Says: import conflicts with {noun} `{name}`
    ImportConflict { noun: String, name: String },
    /// Says: `{product}` is given `@where({dimension}={previous})` here, but the body reads it with `@where({dimension}={value})`
    WhereConflictsWithBody {
        product: String,
        dimension: String,
        previous: String,
        value: String,
    },
    /// Says: `{product}` is given `@same(...)` here, and the body reads it with another; give it once
    SameConflictsWithBody { product: String },
    /// Says: operation `{name}` has a body, so it names each output its steps assign, as in `-> (result: Type)`
    BodyOutputsUnnamed { name: String },
    /// Says: output `{port}` of `{name}` takes its extension, folder and place from the step that writes it; write only its name and type
    BodyOutputDeclaresPlace { port: String, name: String },
    /// Says: operation `{name}` has a body; write `@min(...)` on the `many` input of the step that collects
    BodyHasMinimum { name: String },
    /// Says: operation `{called}` must be declared before `{name}`, whose body calls it
    BodyCallsUndeclared { called: String, name: String },
    /// Says: the body of `{name}` reads `{product}`, which is neither one of its inputs nor made by an earlier step of it
    BodyReadsUnknown { name: String, product: String },
    /// Says: the body of `{name}` already has `{output}`; name each product once
    BodyRepeatsProduct { name: String, output: String },
    /// Says: no step in the body of `{name}` makes its output `{output}`
    BodyOutputUnmade { name: String, output: String },
    /// Says: duplicate operation `{name}`
    DuplicateOperation {
        name: String,
        line: usize,
        imported: bool,
        global: bool,
    },
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePathTemplateProduct { product } => write!(f, "duplicate path template for product `{product}`"),
            Self::DuplicateDefaultPathTemplateStage { stage } => write!(f, "duplicate default path template for stage `{stage}`"),
            Self::DuplicateDefaultPathTemplate => f.write_str("duplicate default path template"),
            Self::DuplicateDefaultExt { whose } => write!(f, "duplicate `default ext:`{whose}"),
            Self::DuplicateDefaultChecks { whose } => write!(f, "duplicate `default check:` list{whose}; write the checks in one line"),
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
            Self::DuplicateDefaultReviews { whose } => write!(f, "duplicate `default review:` list{whose}; write the reviews in one line"),
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
            Self::DuplicateDefaultEntities { scope } => write!(f, "duplicate `default entities:`{scope}"),
            Self::EntityLabelWithoutSpelling { scope } => write!(f, "an entity label renames a dimension in a `default entities:` spelling, and there is none{scope}; add one, as `default entities: {{@key}}={{@value}} separated \"__\"`"),
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
        }
    }
}
