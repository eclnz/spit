//! The errors of tools; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `tools`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolError {
    /// Reads: expected `tool name`, as in `tool fsl`, or names separated by commas
    ExpectedToolNames,
    /// Reads: tool `{name}` is named twice
    ToolNamedTwice { name: String },
    /// Reads: expected `anchored operation`, as in `anchored feat`, or operations separated by commas
    ExpectedAnchoredNames,
    /// Reads: operation `{name}` is named twice
    OperationNamedTwice { name: String },
    /// Reads: expected `uses operation: tools`, as in `uses register: fsl, !mrtrix3`
    ExpectedUsesForm,
    /// Reads: a `uses` list names one or more tools, as in `fsl, !mrtrix3`
    EmptyUsesList,
    /// Reads: a comma in a `uses` list separates two tools, as in `fsl, !mrtrix3`
    EmptyUsesItem,
    /// Reads: tool `{name}` is named twice in one `uses` list
    ToolRepeatedInUses { name: String },
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedToolNames => f.write_str("expected `tool name`, as in `tool fsl`, or names separated by commas"),
            Self::ToolNamedTwice { name } => write!(f, "tool `{name}` is named twice"),
            Self::ExpectedAnchoredNames => f.write_str("expected `anchored operation`, as in `anchored feat`, or operations separated by commas"),
            Self::OperationNamedTwice { name } => write!(f, "operation `{name}` is named twice"),
            Self::ExpectedUsesForm => f.write_str("expected `uses operation: tools`, as in `uses register: fsl, !mrtrix3`"),
            Self::EmptyUsesList => f.write_str("a `uses` list names one or more tools, as in `fsl, !mrtrix3`"),
            Self::EmptyUsesItem => f.write_str("a comma in a `uses` list separates two tools, as in `fsl, !mrtrix3`"),
            Self::ToolRepeatedInUses { name } => write!(f, "tool `{name}` is named twice in one `uses` list"),
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
            ToolError::ExpectedToolNames,
            ToolError::ToolNamedTwice {
                name: "name".to_owned(),
            },
            ToolError::ExpectedAnchoredNames,
            ToolError::OperationNamedTwice {
                name: "name".to_owned(),
            },
            ToolError::ExpectedUsesForm,
            ToolError::EmptyUsesList,
            ToolError::EmptyUsesItem,
            ToolError::ToolRepeatedInUses {
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
