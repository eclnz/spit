//! Errors the parser reads from `operation`; [`fmt::Display`] words each.

use crate::model::DEFAULT_OUTPUT;
use std::fmt;

/// The errors of `operation`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationError {
    /// Says: expected `(` after operation name
    ExpectedAfterOperationName,
    /// Says: expected `->` before operation output type
    ExpectedBeforeOperationOutputType,
    /// Says: `@{keyword}(...)` follows the port it applies to, as in `(image: Image @{keyword}(name))` or `-> (mask: Mask @{keyword}(name))`
    FollowsPortAppliesImageImageName { keyword: String },
    /// Says: an operation does not name the dimensions it collects, its calls do: remove                  `@drop({vary})` and write `@vary({vary})` on the many input of each call, as in                  `result = {name}(input @vary({vary}))`
    OperationDoesNotNameDimensionsCollects { vary: String, name: String },
    /// Says: operation needs at least one input
    OperationNeedsLeastInput,
    /// Says: `@min(count)` requires a many input
    MinCountRequiresManyInput,
    /// Says: `@drop(...)` needs a dimension
    DropNeedsDimension,
    /// Says: `@drop(...)` repeats `{dimension}`
    DropRepeats { dimension: String },
    /// Says: `@min(count)` needs a positive integer
    MinCountNeedsPositiveInteger,
    /// Says: duplicate `@{keyword}(...)`
    Duplicate { keyword: String },
    /// Says: input port name `{DEFAULT_OUTPUT}` is reserved for the operation output
    InputPortNameReservedOperationOutput,
    /// Says: expected closing `)` after output ports
    ExpectedClosingAfterOutputPorts,
    /// Says: expected at least one output port
    ExpectedLeastOutputPort,
    /// Says: named output port `output` is reserved for the single unnamed output; omit the name and parentheses
    NamedOutputPortOutputReservedSingle,
    /// Says: expected the opening ` "` of the suffix
    ExpectedOpeningSuffix,
    /// Says: ` "{suffix} "` cannot end a file name; use letters, digits, `.`, `-` or `_`, as in ` "_mask.nii.gz "`
    CannotEndFileNameUseLetters { suffix: String },
    /// Says: {article} {role} written beside another is a file, not a folder; drop the `/`
    WrittenBesideAnotherFileNotFolder { article: String, role: String },
    /// Says: {article} {role} written beside another names what its file name ends with, as in `{item} .json beside image`, `{item}  "_mask.nii.gz " beside image` or `{item} +.fai beside fasta`
    WrittenBesideAnotherNamesWhatFile {
        article: String,
        role: String,
        item: String,
    },
    /// Says: write `{rest}:{kind}`, the port's name first
    PortNameFirst { rest: String, kind: String },
    /// Says: an input port needs a name, as in `{suggested}: {kind}{rest}`
    PortNeedsName {
        suggested: String,
        kind: String,
        rest: String,
    },
    /// Says: an input port needs a name, as in `items: many`
    ManyPortNeedsName,
    /// Says: an input port needs a name, as in `item`
    OnePortNeedsName,
    /// Says: an input takes one artifact unless it says `many`; write `{instead}`
    InputTakesOne { instead: String },
    /// Says: `{name}` is written beside itself; name another output or an input of this operation, whose file it follows
    BesideItself { name: String },
    /// Says: `{name}` is written beside `{sibling}`, a `many` input, but one job reads many `{sibling}` files and writes one `{name}`; name a `one` input
    BesideManyInput { name: String, sibling: String },
    /// Says: `{name}` is written beside `{sibling}`, which is neither an input nor an output of this operation
    BesideNeither { name: String, sibling: String },
    /// Says: `{name}` is written beside `{sibling}`, which is a folder, and only a file has files beside it; drop `beside {sibling}` and give the tool `{{name}}`
    BesideFolder { name: String, sibling: String },
    /// Says: `{name}` is written beside `{sibling}`, which is itself written beside another; name an output the tool is told to write
    BesideNested { name: String, sibling: String },
    /// Says: `{name}` is written beside `{sibling}`, which declares no extension for `{name}` to replace; give `{sibling}` one, as in `{sibling}: Image .nii.gz`, or append to its whole name with `+`
    BesideNoExtension { name: String, sibling: String },
    /// Says: `@min(count)` goes on a `many` input; `{port_name}` takes one artifact
    MinCountGoesManyInputTakes { port_name: String },
    /// Says: write a many input's minimum beside it: `{name}({list}){outputs}`
    WriteManyInputSMinimumBeside {
        name: String,
        list: String,
        outputs: String,
    },
}

impl fmt::Display for OperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedAfterOperationName => f.write_str("expected `(` after operation name"),
            Self::ExpectedBeforeOperationOutputType => f.write_str("expected `->` before operation output type"),
            Self::FollowsPortAppliesImageImageName { keyword } => write!(f, "`@{keyword}(...)` follows the port it applies to, as in `(image: Image @{keyword}(name))` or `-> (mask: Mask @{keyword}(name))`"),
            Self::OperationDoesNotNameDimensionsCollects { vary, name } => write!(f, "an operation does not name the dimensions it collects, its calls do: remove \
                 `@drop({vary})` and write `@vary({vary})` on the many input of each call, as in \
                 `result = {name}(input @vary({vary}))`"),
            Self::OperationNeedsLeastInput => f.write_str("operation needs at least one input"),
            Self::MinCountRequiresManyInput => f.write_str("`@min(count)` requires a many input"),
            Self::DropNeedsDimension => f.write_str("`@drop(...)` needs a dimension"),
            Self::DropRepeats { dimension } => write!(f, "`@drop(...)` repeats `{dimension}`"),
            Self::MinCountNeedsPositiveInteger => f.write_str("`@min(count)` needs a positive integer"),
            Self::Duplicate { keyword } => write!(f, "duplicate `@{keyword}(...)`"),
            Self::InputPortNameReservedOperationOutput => write!(f, "input port name `{DEFAULT_OUTPUT}` is reserved for the operation output"),
            Self::ExpectedClosingAfterOutputPorts => f.write_str("expected closing `)` after output ports"),
            Self::ExpectedLeastOutputPort => f.write_str("expected at least one output port"),
            Self::NamedOutputPortOutputReservedSingle => f.write_str("named output port `output` is reserved for the single unnamed output; omit the name and parentheses"),
            Self::ExpectedOpeningSuffix => f.write_str("expected the opening `\"` of the suffix"),
            Self::CannotEndFileNameUseLetters { suffix } => write!(f, "`\"{suffix}\"` cannot end a file name; use letters, digits, `.`, `-` or `_`, as in `\"_mask.nii.gz\"`"),
            Self::WrittenBesideAnotherFileNotFolder { article, role } => write!(f, "{article} {role} written beside another is a file, not a folder; drop the `/`"),
            Self::WrittenBesideAnotherNamesWhatFile { article, role, item } => write!(f, "{article} {role} written beside another names what its file name ends with, as in `{item} .json beside image`, `{item} \"_mask.nii.gz\" beside image` or `{item} +.fai beside fasta`"),
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
            Self::MinCountGoesManyInputTakes { port_name } => write!(f, "`@min(count)` goes on a `many` input; `{port_name}` takes one artifact"),
            Self::WriteManyInputSMinimumBeside { name, list, outputs } => write!(f, "write a many input's minimum beside it: `{name}({list}){outputs}`"),
        }
    }
}
