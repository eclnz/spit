use crate::model::DEFAULT_OUTPUT;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationError {
    ExpectedAfterOperationName,
    ExpectedArrow,
    ClauseAfterWrongPlace {
        keyword: String,
    },
    DropOnOperation {
        vary: String,
        name: String,
    },
    NoInputs,
    MinCountRequiresManyInput,
    DropWithoutDimension,
    DropRepeats {
        dimension: String,
    },
    MinNotPositive,
    PortNameReserved,
    UnclosedOutputPorts,
    NoOutputPorts,
    OutputPortNameReserved,
    ExpectedOpeningSuffix,
    BadSuffix {
        suffix: String,
    },
    NamingNeedsExtension {
        naming: String,
    },
    QuotedSuffixReplaced {
        written: String,
        replacement: String,
    },
    BesideFolderKind {
        article: String,
        role: String,
    },
    BesideNeedsSuffix {
        article: String,
        role: String,
        item: String,
    },
    PortNameFirst {
        rest: String,
        kind: String,
    },
    PortNeedsName {
        suggested: String,
        kind: String,
        rest: String,
    },
    ManyPortNeedsName,
    OnePortNeedsName,
    InputTakesOne {
        instead: String,
    },
    BesideItself {
        name: String,
    },
    BesideManyInput {
        name: String,
        sibling: String,
    },
    BesideNeither {
        name: String,
        sibling: String,
    },
    BesideFolder {
        name: String,
        sibling: String,
    },
    BesideNested {
        name: String,
        sibling: String,
    },
    BesideNoExtension {
        name: String,
        sibling: String,
    },
    MinOnOneInput {
        port_name: String,
    },
    MinimumBesideInput {
        name: String,
        list: String,
        outputs: String,
    },
}

impl fmt::Display for OperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedAfterOperationName => f.write_str("expected `(` after operation name"),
            Self::ExpectedArrow => f.write_str("expected `->` before operation output type"),
            Self::ClauseAfterWrongPlace { keyword } => write!(f, "`@{keyword}(...)` follows the port it applies to, as in `(image: Image @{keyword}(name))` or `-> (mask: Mask @{keyword}(name))`"),
            Self::DropOnOperation { vary, name } => write!(f, "an operation does not name the dimensions it collects, its calls do: remove \
                 `@drop({vary})` and write `@vary({vary})` on the many input of each call, as in \
                 `result = {name}(input @vary({vary}))`"),
            Self::NoInputs => f.write_str("operation needs at least one input"),
            Self::MinCountRequiresManyInput => f.write_str("`@min(count)` requires a many input"),
            Self::DropWithoutDimension => f.write_str("`@drop(...)` needs a dimension"),
            Self::DropRepeats { dimension } => write!(f, "`@drop(...)` repeats `{dimension}`"),
            Self::MinNotPositive => f.write_str("`@min(count)` needs a positive integer"),
            Self::PortNameReserved => write!(f, "input port name `{DEFAULT_OUTPUT}` is reserved for the operation output"),
            Self::UnclosedOutputPorts => f.write_str("expected closing `)` after output ports"),
            Self::NoOutputPorts => f.write_str("expected at least one output port"),
            Self::OutputPortNameReserved => f.write_str("named output port `output` is reserved for the single unnamed output; omit the name and parentheses"),
            Self::ExpectedOpeningSuffix => f.write_str("expected the suffix in quotes, as in `suffix \"_mask\"`"),
            Self::BadSuffix { suffix } => write!(f, "`\"{suffix}\"` cannot be part of a file name; use letters, digits, `.`, `-` or `_`, as in `suffix \"_mask\"`"),
            Self::NamingNeedsExtension { naming } => write!(f, "`suffix \"{naming}\"` goes before an extension, so the output needs a file extension, as in `.nii.gz suffix \"{naming}\"`"),
            Self::QuotedSuffixReplaced { written, replacement } => write!(f, "a quoted suffix `\"{written}\"` names the extension and the naming suffix together; write the extension, then `suffix` after the sibling, as `{replacement}`"),
            Self::BesideFolderKind { article, role } => write!(f, "{article} {role} written beside another is a file, not a folder; drop the `/`"),
            Self::BesideNeedsSuffix { article, role, item } => write!(f, "{article} {role} written beside another names what its file name ends with, as in `{item} .json beside image`, `{item} \"_mask.nii.gz\" beside image` or `{item} +.fai beside fasta`"),
            Self::PortNameFirst { rest, kind } => write!(f, "write `{rest}:{kind}`, the port's name first"),
            Self::PortNeedsName { suggested, kind, rest } => write!(f, "an input port needs a name, as in `{suggested}: {kind}{rest}`"),
            Self::ManyPortNeedsName => f.write_str("an input port needs a name, as in `items: many`"),
            Self::OnePortNeedsName => f.write_str("an input port needs a name, as in `item`"),
            Self::InputTakesOne { instead } => write!(f, "an input takes one artifact unless it says `many`; write `{instead}`"),
            Self::BesideItself { name } => write!(f, "`{name}` is written beside itself; name another output or an input of this operation, whose file it follows"),
            Self::BesideManyInput { name, sibling } => write!(f, "`{name}` is written beside `{sibling}`, a `many` input, but one job reads many `{sibling}` files and writes one `{name}`; name a `one` input"),
            Self::BesideNeither { name, sibling } => write!(f, "`{name}` is written beside `{sibling}`, which is neither an input nor an output of this operation"),
            Self::BesideFolder { name, sibling } => write!(f, "`{name}` is written beside `{sibling}`, which is a folder, and only a file has files beside it; drop `beside {sibling}` and give the tool `{{{name}}}`"),
            Self::BesideNested { name, sibling } => write!(f, "`{name}` is written beside `{sibling}`, which is itself written beside another; name an output the tool is told to write"),
            Self::BesideNoExtension { name, sibling } => write!(f, "`{name}` is written beside `{sibling}`, which declares no extension for `{name}` to replace; give `{sibling}` one, as in `{sibling}: Image .nii.gz`, or append to its whole name with `+`"),
            Self::MinOnOneInput { port_name } => write!(f, "`@min(count)` goes on a `many` input; `{port_name}` takes one artifact"),
            Self::MinimumBesideInput { name, list, outputs } => write!(f, "write a many input's minimum beside it: `{name}({list}){outputs}`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            OperationError::ExpectedAfterOperationName,
            OperationError::ExpectedArrow,
            OperationError::ClauseAfterWrongPlace {
                keyword: "keyword".to_owned(),
            },
            OperationError::DropOnOperation {
                vary: "vary".to_owned(),
                name: "name".to_owned(),
            },
            OperationError::NoInputs,
            OperationError::MinCountRequiresManyInput,
            OperationError::DropWithoutDimension,
            OperationError::DropRepeats {
                dimension: "dimension".to_owned(),
            },
            OperationError::MinNotPositive,
            OperationError::PortNameReserved,
            OperationError::UnclosedOutputPorts,
            OperationError::NoOutputPorts,
            OperationError::OutputPortNameReserved,
            OperationError::ExpectedOpeningSuffix,
            OperationError::BadSuffix {
                suffix: "suffix".to_owned(),
            },
            OperationError::NamingNeedsExtension {
                naming: "naming".to_owned(),
            },
            OperationError::QuotedSuffixReplaced {
                written: "written".to_owned(),
                replacement: "replacement".to_owned(),
            },
            OperationError::BesideFolderKind {
                article: "article".to_owned(),
                role: "role".to_owned(),
            },
            OperationError::BesideNeedsSuffix {
                article: "article".to_owned(),
                role: "role".to_owned(),
                item: "item".to_owned(),
            },
            OperationError::PortNameFirst {
                rest: "rest".to_owned(),
                kind: "kind".to_owned(),
            },
            OperationError::PortNeedsName {
                suggested: "suggested".to_owned(),
                kind: "kind".to_owned(),
                rest: "rest".to_owned(),
            },
            OperationError::ManyPortNeedsName,
            OperationError::OnePortNeedsName,
            OperationError::InputTakesOne {
                instead: "instead".to_owned(),
            },
            OperationError::BesideItself {
                name: "name".to_owned(),
            },
            OperationError::BesideManyInput {
                name: "name".to_owned(),
                sibling: "sibling".to_owned(),
            },
            OperationError::BesideNeither {
                name: "name".to_owned(),
                sibling: "sibling".to_owned(),
            },
            OperationError::BesideFolder {
                name: "name".to_owned(),
                sibling: "sibling".to_owned(),
            },
            OperationError::BesideNested {
                name: "name".to_owned(),
                sibling: "sibling".to_owned(),
            },
            OperationError::BesideNoExtension {
                name: "name".to_owned(),
                sibling: "sibling".to_owned(),
            },
            OperationError::MinOnOneInput {
                port_name: "port_name".to_owned(),
            },
            OperationError::MinimumBesideInput {
                name: "name".to_owned(),
                list: "list".to_owned(),
                outputs: "outputs".to_owned(),
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
