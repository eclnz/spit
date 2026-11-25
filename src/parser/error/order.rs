use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrderError {
    SourceOrderDisagrees {
        name: String,
        dimensions: String,
        ordered: String,
    },
    DimensionsUnordered {
        name: String,
        first: String,
        second: String,
        suggestion: String,
    },
    ProductOrderDisagrees {
        name: String,
        dimensions: String,
        sorted: String,
    },
    SourcesOrderDifferently {
        name: String,
        first: String,
        second: String,
        by: String,
    },
    DimensionsNamesUnknown {
        unknown: String,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            OrderError::SourceOrderDisagrees {
                name: "name".to_owned(),
                dimensions: "dimensions".to_owned(),
                ordered: "ordered".to_owned(),
            },
            OrderError::DimensionsUnordered {
                name: "name".to_owned(),
                first: "first".to_owned(),
                second: "second".to_owned(),
                suggestion: "suggestion".to_owned(),
            },
            OrderError::ProductOrderDisagrees {
                name: "name".to_owned(),
                dimensions: "dimensions".to_owned(),
                sorted: "sorted".to_owned(),
            },
            OrderError::SourcesOrderDifferently {
                name: "name".to_owned(),
                first: "first".to_owned(),
                second: "second".to_owned(),
                by: "by".to_owned(),
            },
            OrderError::DimensionsNamesUnknown {
                unknown: "unknown".to_owned(),
            },
            OrderError::DimensionsLeavesOut {
                missing: "missing".to_owned(),
                owner: Some("owner".to_owned()),
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

    fn held_text(debug: &str) -> Vec<&str> {
        debug.split('"').skip(1).step_by(2).collect()
    }
}
