mod body;
mod calculation;
mod check;
mod command;
mod declarations;
mod entities;
pub(crate) mod error;
mod flow;
mod inventory;
mod keyword;
mod layout;
mod lexical;
mod operation;
mod render_inventory;
mod resources;
mod review;
mod rules;
mod scan;
mod source_map;
mod tools;

pub(crate) use entities::EntitiesDeclaration;
pub use error::{ClauseSite, ParseError, ParseErrorKind};

use std::ops::Range;

use crate::model::{
    CheckDef, CheckKind, CommandDef, CommandRole, CoverageRule, DefaultChecks, Invocation,
    OperationDef, ProductDef, ResourcesDef, StepOutput, ToolList, TypeChecking, UsesDef,
};
use crate::span::{content_columns, Place};
use crate::template::PathTemplate;

use self::flow::parse_flow;

pub(crate) use self::body::empty_body;
pub(crate) use self::declarations::{parse_use, UseSpec};
pub use self::inventory::parse_source_inventory;
pub(crate) use self::inventory::{
    document_records, parse_inventory_recovering, source_record_lines,
};
pub use self::keyword::keywords_json;
pub(crate) use self::keyword::{File, Header, Keyword};
pub(crate) use self::layout::{Block, Head, Layout, Line};
pub(crate) use self::lexical::without_bom;
pub(crate) use self::render_inventory::as_read_back;
pub use self::render_inventory::render_source_inventory;
pub(crate) use self::source_map::{CommandPlaces, Rule, SourceMap, Step};

#[derive(Clone, Debug, Default)]
pub(crate) struct Syntax {
    pub(crate) statements: Vec<Statement>,
    pub(crate) errors: Vec<(usize, ParseError)>,
}

#[derive(Clone, Debug)]
pub(crate) struct Statement {
    pub(crate) place: Place,
    pub(crate) kind: StatementKind,
}

#[derive(Clone, Debug)]
pub(crate) enum StatementKind {
    Import,
    Root(std::path::PathBuf),
    OutputRoot(std::path::PathBuf),
    Stage {
        name: String,
        place: Place,
    },
    Product(ProductDef, Place),
    Operation(OperationDef, Place, Option<String>, Range<usize>),
    Constraint(CoverageRule, Rule),
    Exclude(declarations::ExcludeRule, Option<String>, Place),
    Prefer(crate::template::PathGlob, Option<String>),
    Command(CommandDef, CommandPlaces),
    Resources(ResourcesDef, Place),
    Tool(Vec<String>, Place),
    Uses(UsesDef, Place),
    Anchored(Vec<String>, Place),
    DefaultUses {
        stage: Option<String>,
        tools: ToolList,
        place: Place,
    },
    Check(CheckDef, Place),
    ReviewFile {
        review: String,
        file: String,
        place: Place,
    },
    Dimensions(Vec<String>),
    Types(TypeChecking),
    TypeDecl(String, Vec<String>),
    Entities {
        stage: Option<String>,
        declaration: EntitiesDeclaration,
    },
    Path(PathRule),
    Extension {
        stage: Option<String>,
        extension: String,
    },
    DefaultChecks {
        stage: Option<String>,
        extension: Option<String>,
        checks: DefaultChecks,
        place: Place,
    },
    FlowStep(FlowStep),
}

#[derive(Clone, Debug)]
pub(crate) struct PathRule {
    pub(crate) product: Option<String>,
    pub(crate) stage: Option<String>,
    pub(crate) template: PathTemplate,
    pub(crate) place: Place,
}

#[derive(Clone, Debug)]
pub(crate) struct FlowStep {
    pub(crate) invocation: Invocation,
    pub(crate) outputs: Vec<StepOutput>,
    pub(crate) step: Step,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Pipeline,
    Recipe,
}

pub(crate) fn parse_syntax(layout: &Layout) -> Syntax {
    parse_flow(layout)
}

impl StatementKind {
    fn product(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let product = declarations::parse_product(declaration, number)?;
        let place = source_map::name_place(original, number, declaration, &product.name);
        Ok(Self::Product(product, place))
    }

    fn operation(
        original: &str,
        declaration: &str,
        number: usize,
        stage: Option<String>,
    ) -> Result<Self, ParseError> {
        let declaration = declaration.trim();
        let signature = declaration
            .strip_suffix(':')
            .unwrap_or(declaration)
            .trim_end();
        let operation = operation::parse_operation(signature, number)?;
        let place = source_map::name_place(original, number, signature, &operation.name);
        Ok(Self::Operation(
            operation,
            place.clone(),
            stage,
            place.columns,
        ))
    }

    fn constraint(
        original: &str,
        keyword: Keyword,
        rest: &str,
        number: usize,
    ) -> Result<Self, ParseError> {
        let rule = rules::parse_coverage_rule(keyword, rest, number)?;
        let place = source_map::rule_place(original, number, &rule);
        Ok(Self::Constraint(rule, place))
    }

    fn exclude(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let rule = declarations::parse_exclude(declaration, number)?;
        let place = source_map::tail_place(original, number, declaration.trim());
        let reason = crate::span::comment_text(original).map(str::to_owned);
        Ok(Self::Exclude(rule, reason, place))
    }

    fn prefer(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let path = declarations::parse_prefer(declaration, number)?;
        let reason = crate::span::comment_text(original).map(str::to_owned);
        Ok(Self::Prefer(path, reason))
    }

    fn check(
        original: &str,
        declaration: &str,
        number: usize,
        kind: CheckKind,
    ) -> Result<Self, ParseError> {
        let mut check = check::parse_check(declaration, number)?;
        check.kind = kind;
        let place = source_map::name_place(original, number, declaration, &check.name);
        Ok(Self::Check(check, place))
    }

    fn review(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        Ok(match review::parse_review(declaration, number)? {
            review::ReviewLine::File { review, file } => {
                let place = source_map::name_place(original, number, declaration, &review);
                Self::ReviewFile {
                    review,
                    file,
                    place,
                }
            }
        })
    }

    fn command(
        original: &str,
        declaration: &str,
        number: usize,
        role: CommandRole,
    ) -> Result<Self, ParseError> {
        let (command, places) = command::parse_command(original, declaration, number, role)?;
        Ok(Self::Command(command, places))
    }

    fn resources(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let resources = resources::parse_resources(declaration, number)?;
        let place = source_map::name_place(original, number, declaration, &resources.operation);
        Ok(Self::Resources(resources, place))
    }

    fn tool(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let names = tools::parse_tool_names(declaration, number)?;
        Ok(Self::Tool(
            names,
            source_map::tail_place(original, number, declaration),
        ))
    }

    fn anchored(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let names = tools::parse_anchored(declaration, number)?;
        Ok(Self::Anchored(
            names,
            source_map::tail_place(original, number, declaration),
        ))
    }

    fn uses(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let uses = tools::parse_uses(declaration, number)?;
        let place = source_map::name_place(original, number, declaration, &uses.operation);
        Ok(Self::Uses(uses, place))
    }
}

impl Syntax {
    fn push(&mut self, original: &str, number: usize, kind: StatementKind) {
        self.statements.push(Statement {
            place: Place::new(number, content_columns(original)),
            kind,
        });
    }

    fn fail(&mut self, error: ParseError) {
        self.errors.push((self.statements.len(), error));
    }
}
