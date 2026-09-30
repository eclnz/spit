use std::fmt;

use crate::model::{ArtifactInstance, ArtifactType, CountRequirement, EntityBinding, SourceRecord};

/// The declaration an [`ResolveError::InvalidDefinition`] refers to, so callers
/// such as editor diagnostics can locate it without parsing the message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DefinitionSubject {
    Product(String),
    Operation(String),
    /// An invocation, named by the product it produces.
    Invocation(String),
    /// A coverage rule, by its index in `Pipeline::constraints`.
    Constraint(usize),
    /// The dimensions a coverage rule groups by, by the rule's index.
    ConstraintGroup(usize),
    Source(SourceRecord),
    /// A stage, by name.
    Stage(String),
    None,
}

/// A type variable, the type it was inferred as, and the different type a
/// later port requires of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeConflict {
    pub variable: String,
    pub previous: ArtifactType,
    pub required: ArtifactType,
}

/// Where an input is bound: the step, by its operation and first output,
/// the operation's port, and the product bound to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortSite {
    pub operation: String,
    pub output_product: String,
    pub port: String,
    pub product: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolveError {
    UnknownProduct {
        name: String,
    },
    UnknownOperation {
        name: String,
    },
    TypeMismatch {
        site: PortSite,
        expected: Box<ArtifactType>,
        found: Box<ArtifactType>,
    },
    TypeVariableConflict {
        site: PortSite,
        conflict: Box<TypeConflict>,
    },
    MissingInput {
        site: PortSite,
        context: Box<EntityBinding>,
    },
    /// A `one` input left more than one artifact for a job.
    AmbiguousInput {
        site: PortSite,
        context: Box<EntityBinding>,
    },
    /// A many input collected fewer artifacts than the operation accepts.
    CollectionTooSmall {
        site: PortSite,
        context: Box<EntityBinding>,
        minimum: usize,
        found: usize,
    },
    InvalidAggregationDimension {
        product: String,
        dimension: String,
    },
    DuplicateSourceArtifact {
        artifact: ArtifactInstance,
    },
    DuplicateOutputArtifact {
        artifact: ArtifactInstance,
    },
    Cycle {
        products: Vec<String>,
    },
    UnsupportedShapeRelationship {
        operation: String,
        detail: String,
    },
    CoverageViolation {
        product: String,
        rule_index: usize,
        context: EntityBinding,
        expected: CountRequirement,
        found: usize,
        discovery: bool,
    },
    /// A group lacks an entity value its coverage rule requires.
    MissingRequiredValue {
        product: String,
        rule_index: usize,
        context: EntityBinding,
        dimension: String,
        value: String,
        discovery: bool,
    },
    InvalidDefinition {
        subject: DefinitionSubject,
        detail: String,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProduct { name } => write!(f, "unknown product `{name}`"),
            Self::UnknownOperation { name } => write!(f, "unknown operation `{name}`"),
            Self::TypeMismatch {
                site:
                    PortSite {
                        operation,
                        port,
                        product,
                        ..
                    },
                expected,
                found,
            } => write!(
                f,
                "type mismatch at `{operation}.{port}`: product `{product}` is {found}, expected {expected}"
            ),
            Self::TypeVariableConflict {
                site:
                    PortSite {
                        operation,
                        port,
                        product,
                        ..
                    },
                conflict,
            } => write!(
                f,
                "type conflict at `{operation}.{port}` (product `{product}`): variable `{}` was inferred as {}, but now requires {}",
                conflict.variable,
                conflict.previous,
                conflict.required
            ),
            Self::MissingInput {
                site:
                    PortSite {
                        operation,
                        port,
                        product,
                        ..
                    },
                context,
            } => write!(
                f,
                "no `{product}` artifact for input `{port}` of `{operation}` at [{context}]"
            ),
            Self::AmbiguousInput {
                site:
                    PortSite {
                        operation,
                        port,
                        product,
                        ..
                    },
                context,
            } => write!(
                f,
                "more than one `{product}` artifact matches input `{port}` of `{operation}` at [{context}]; select one with `@ where(...)`"
            ),
            Self::CollectionTooSmall {
                site: PortSite {
                    operation, port, ..
                },
                context,
                minimum,
                found,
            } => write!(
                f,
                "input `{port}` of `{operation}` needs at least {minimum} artifacts at [{context}], found {found}"
            ),
            Self::InvalidAggregationDimension { product, dimension } => write!(
                f,
                "cannot aggregate `{product}` over absent dimension `{dimension}`"
            ),
            Self::DuplicateSourceArtifact { artifact } => {
                write!(f, "duplicate source artifact `{artifact}`")
            }
            Self::DuplicateOutputArtifact { artifact } => {
                write!(f, "duplicate output artifact `{artifact}`")
            }
            Self::Cycle { products } => {
                write!(f, "pipeline cycle: {}", products.join(" -> "))
            }
            Self::UnsupportedShapeRelationship { operation, detail } => {
                write!(f, "unsupported shape for `{operation}`: {detail}")
            }
            Self::CoverageViolation {
                product,
                context,
                expected,
                found,
                discovery,
                ..
            } => if *discovery {
                write!(f, "discovery coverage for `{product}` at [{context}]: expected {expected} binding(s), found {found}")
            } else {
                write!(f, "source coverage for `{product}` at [{context}]: expected {expected} artifact(s), found {found}")
            },
            Self::MissingRequiredValue {
                product,
                context,
                dimension,
                value,
                discovery,
                ..
            } => if *discovery {
                write!(f, "discovery coverage for `{product}` at [{context}]: no binding with {dimension}={value}")
            } else {
                write!(f, "source coverage for `{product}` at [{context}]: no artifact with {dimension}={value}")
            },
            Self::InvalidDefinition { detail, .. } => f.write_str(detail),
        }
    }
}

impl std::error::Error for ResolveError {}
