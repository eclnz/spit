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
    None,
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
        operation: String,
        output_product: String,
        port: String,
        product: String,
        expected: Box<ArtifactType>,
        found: Box<ArtifactType>,
    },
    TypeVariableConflict {
        operation: String,
        output_product: String,
        port: String,
        variable: String,
        previous: Box<ArtifactType>,
        required: Box<ArtifactType>,
    },
    MissingInput {
        operation: String,
        output_product: String,
        port: String,
        context: EntityBinding,
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
                operation,
                port,
                product,
                expected,
                found,
                ..
            } => write!(
                f,
                "type mismatch at `{operation}.{port}`: product `{product}` is {found}, expected {expected}"
            ),
            Self::TypeVariableConflict {
                operation,
                port,
                variable,
                previous,
                required,
                ..
            } => write!(
                f,
                "type conflict at `{operation}.{port}`: variable `{variable}` was inferred as {previous}, but now requires {required}"
            ),
            Self::MissingInput {
                operation,
                port,
                context,
                ..
            } => write!(
                f,
                "missing input `{port}` for `{operation}` at [{context}]"
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
                ..
            } => write!(
                f,
                "source coverage for `{product}` at [{context}]: expected {expected} artifact(s), found {found}"
            ),
            Self::InvalidDefinition { detail, .. } => f.write_str(detail),
        }
    }
}

impl std::error::Error for ResolveError {}
