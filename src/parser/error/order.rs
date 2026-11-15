//! Errors the parser reads from `order`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `order`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrderError {
    /// Says: `{name}` lists its dimensions as \[{dimensions}\], but `dimensions` orders them \[{ordered}\]
    SourceOrderDisagrees {
        name: String,
        dimensions: String,
        ordered: String,
    },
    /// Says: `{name}` has dimensions `{first}` and `{second}`, which no source orders;                      declare the order once with `dimensions \[{suggestion}\]`
    DimensionsUnordered {
        name: String,
        first: String,
        second: String,
        suggestion: String,
    },
    /// Says: `{name}` lists its dimensions as \[{dimensions}\], but the pipeline orders them \[{sorted}\]
    ProductOrderDisagrees {
        name: String,
        dimensions: String,
        sorted: String,
    },
    /// Says: `{name}` lists `{first}` before `{second}`, but {by} `{second}` first;              list them in one order, or declare it with `dimensions \[...\]`
    SourcesOrderDifferently {
        name: String,
        first: String,
        second: String,
        by: String,
    },
    /// Says: `dimensions` names `{unknown}`, which no product has
    DimensionsNamesUnknown { unknown: String },
    /// Says: `dimensions` leaves out `{missing}`
    DimensionsLeavesOut {
        missing: String,
        owner: Option<String>,
    },
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceOrderDisagrees { name, dimensions, ordered } => write!(f, "`{name}` lists its dimensions as [{dimensions}], but `dimensions` orders them [{ordered}]"),
            Self::DimensionsUnordered { name, first, second, suggestion } => write!(f, "`{name}` has dimensions `{first}` and `{second}`, which no source orders; \
                     declare the order once with `dimensions [{suggestion}]`"),
            Self::ProductOrderDisagrees { name, dimensions, sorted } => write!(f, "`{name}` lists its dimensions as [{dimensions}], but the pipeline orders them [{sorted}]"),
            Self::SourcesOrderDifferently { name, first, second, by } => write!(f, "`{name}` lists `{first}` before `{second}`, but {by} `{second}` first; \
             list them in one order, or declare it with `dimensions [...]`"),
            Self::DimensionsNamesUnknown { unknown } => write!(f, "`dimensions` names `{unknown}`, which no product has"),
            Self::DimensionsLeavesOut { missing, owner } => {
                write!(f, "`dimensions` leaves out `{missing}`")?;
                if let Some(owner) = owner {
                    write!(f, ", a dimension of `{owner}`")?;
                }
                Ok(())
            },
        }
    }
}
