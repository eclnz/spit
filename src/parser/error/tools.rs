use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolError {
    ExpectedToolNames,
    ToolNamedTwice { name: String },
    ExpectedAnchoredNames,
    OperationNamedTwice { name: String },
    ExpectedUsesForm,
    EmptyUsesList,
    EmptyUsesItem,
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
        crate::parser::error::assert_worded(variants);
    }
}
