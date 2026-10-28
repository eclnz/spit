//! What a line starts with: a keyword such as `source` or `path`, or a
//! records header such as `sources:` or `contexts name:`. Every reader of
//! a document classifies its lines here, so they agree on what each is.

/// A keyword that starts a statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Keyword {
    Use,
    Root,
    Source,
    Operation,
    Command,
    Verify,
    /// `resources operation: cores=8, memory=16G`, what each job of an
    /// operation needs.
    Resources,
    /// `check name(params): command`, a test of one artifact.
    Check,
    /// `check:`, the checks every output in a file or stage runs, written
    /// after `default` or in a `default:` block.
    Checks,
    Require,
    /// `skip` and `drop`, kept to give migration errors.
    Skip,
    Drop,
    Exclude,
    /// `path product:` for one product's rule, and `path:` for the default
    /// one, written after `default` or in a `default:` block.
    Path,
    /// `ext:`, the extension a default path is completed with, written
    /// after `default` or in a `default:` block.
    Ext,
    /// `default path:`, `default ext:` and `default check:`, and `default:`,
    /// which opens a block of them.
    Default,
    Entities,
    Stage,
    /// `dimensions [...]`, the order every product's dimensions follow.
    Dimensions,
    /// `sidecars`, which starts no statement, read to say what to write instead.
    Sidecars,
    /// `shell-source:`, which starts no statement, read to say what to write instead.
    ShellSource,
    /// `types strict` or `types standard`, how much of the pipeline must be typed.
    Types,
    /// `type Name<Param, ...>`, which declares a parameterized type.
    Type,
}

/// Every keyword and its word. Each word is reserved: no product may take
/// it as its name, so the first word of a line says whether it starts a
/// statement.
const WORDS: [(Keyword, &str); 22] = [
    (Keyword::Use, "use"),
    (Keyword::Root, "root"),
    (Keyword::Source, "source"),
    (Keyword::Operation, "operation"),
    (Keyword::Command, "command"),
    (Keyword::Verify, "verify"),
    (Keyword::Resources, "resources"),
    (Keyword::Check, "check"),
    (Keyword::Require, "require"),
    (Keyword::Skip, "skip"),
    (Keyword::Drop, "drop"),
    (Keyword::Exclude, "exclude"),
    (Keyword::Path, "path"),
    (Keyword::Ext, "ext"),
    (Keyword::Default, "default"),
    (Keyword::Entities, "entities"),
    (Keyword::Stage, "stage"),
    (Keyword::Dimensions, "dimensions"),
    (Keyword::Sidecars, "sidecars"),
    (Keyword::ShellSource, "shell-source"),
    (Keyword::Types, "types"),
    (Keyword::Type, "type"),
];

/// The keywords that open a block of their statements when written alone
/// with a `:`, as `source:`, and the word for each.
pub(crate) const BLOCKS: [(Keyword, &str); 9] = [
    (Keyword::Source, "source"),
    (Keyword::Operation, "operation"),
    (Keyword::Command, "command"),
    (Keyword::Verify, "verify"),
    (Keyword::Resources, "resources"),
    (Keyword::Check, "check"),
    (Keyword::Path, "path"),
    (Keyword::Require, "require"),
    (Keyword::Exclude, "exclude"),
];

/// Whether `name` is a keyword's word, which no product may take.
pub(crate) fn reserved(name: &str) -> bool {
    WORDS.iter().any(|(_, word)| *word == name)
}

impl Keyword {
    /// The keyword `line` starts with, and the text after its word. A
    /// trimmed line is expected. The first word, up to a space or a `:`,
    /// decides, since every keyword's word is reserved; the text after it
    /// starts after one space. `check` followed by
    /// `:` and a list is the old default list, `Checks`, and `ext:` and
    /// `check:` give the text after their `:`. A keyword of [`BLOCKS`]
    /// alone with its `:` gives `:` as its text.
    pub(crate) fn split(line: &str) -> Option<(Self, &str)> {
        let end = line.find([' ', '\t', ':']).unwrap_or(line.len());
        let (word, rest) = line.split_at(end);
        let &(keyword, _) = WORDS.iter().find(|(_, name)| *name == word)?;
        if rest == ":" && BLOCKS.iter().any(|&(opens, _)| opens == keyword) {
            return Some((keyword, ":"));
        }
        Some(match (keyword, rest.strip_prefix(':')) {
            (Self::Check, Some(list)) => (Self::Checks, list),
            (Self::Ext | Self::ShellSource, Some(after)) => (keyword, after),
            _ => (keyword, rest.strip_prefix(' ').unwrap_or(rest)),
        })
    }
}

/// Whether `text` has an `=` outside parentheses.
pub(super) fn has_top_level_equals(text: &str) -> bool {
    let mut depth = 0usize;
    text.chars().any(|c| {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '=' if depth == 0 => return true,
            _ => {}
        }
        false
    })
}

/// A line that opens a `.spitout`'s records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Header<'a> {
    Sources,
    SourcePaths,
    /// `contexts:`, or `contexts name:` for a named discovery's contexts.
    Contexts(Option<&'a str>),
    /// `removed:`, the record of what the input stage left out.
    Removed,
}

impl<'a> Header<'a> {
    /// The header a trimmed `line` is, if it is one.
    pub(crate) fn of(line: &'a str) -> Option<Self> {
        Some(match line {
            "sources:" => Self::Sources,
            "source_paths:" => Self::SourcePaths,
            "removed:" => Self::Removed,
            "contexts:" => Self::Contexts(None),
            _ => Self::Contexts(Some(
                line.strip_prefix("contexts ")?.strip_suffix(':')?.trim(),
            )),
        })
    }
}

/// The headers of the removed sectioned form, which grouped declarations
/// under `products:` and the like, and what each held. Kept to say what to
/// write instead.
pub(crate) fn removed_section(line: &str) -> Option<&'static str> {
    Some(match line {
        "products:" => "write each source as `source name : Type [dimensions]`, and let each step declare its outputs",
        "operations:" => "write each operation as `operation name(port: Type) -> Type`",
        "pipeline:" => "write each step as `output = operation(inputs)`",
        "commands:" => "write each command as `command operation: program {input} {@output}`",
        "constraints:" => "write each rule on its own line, as `require ...` or `exclude ...`",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{Header, Keyword};

    fn keyword(line: &str) -> Option<Keyword> {
        Keyword::split(line).map(|(keyword, _)| keyword)
    }

    #[test]
    fn lines_are_classified_by_their_first_word() {
        assert_eq!(
            Keyword::split("source raw [id]"),
            Some((Keyword::Source, "raw [id]"))
        );
        assert_eq!(keyword("path: out/{@entities}"), Some(Keyword::Path));
        assert_eq!(
            Keyword::split("default path: out/{@entities}"),
            Some((Keyword::Default, "path: out/{@entities}"))
        );
        assert_eq!(keyword("default ext: .txt"), Some(Keyword::Default));
        assert_eq!(keyword("default check: nonempty"), Some(Keyword::Default));
        assert_eq!(keyword("default:"), Some(Keyword::Default));
        // Every keyword's word is reserved, so a line starting with one
        // starts that statement, even shaped as a step.
        assert_eq!(keyword("default = f(x)"), Some(Keyword::Default));
        assert_eq!(keyword("default: Report = f(x)"), Some(Keyword::Default));
        assert_eq!(keyword("defaults:"), None);
        assert_eq!(Keyword::split("source:"), Some((Keyword::Source, ":")));
        assert_eq!(Keyword::split("check:"), Some((Keyword::Check, ":")));
        assert_eq!(Keyword::split("path:"), Some((Keyword::Path, ":")));
        assert_eq!(
            Keyword::split("resources:"),
            Some((Keyword::Resources, ":"))
        );
        assert_eq!(
            Keyword::split("resources align: cores=8"),
            Some((Keyword::Resources, "align: cores=8"))
        );
        assert_eq!(keyword("check: nonempty"), Some(Keyword::Checks));
        assert_eq!(keyword("stage:"), Some(Keyword::Stage));
        assert_eq!(keyword("path raw: in/{id}"), Some(Keyword::Path));
        assert_eq!(keyword("paths = f(x)"), None);
        assert_eq!(keyword("stage analysis:"), Some(Keyword::Stage));
        assert_eq!(keyword("stage = f(x)"), Some(Keyword::Stage));
        assert_eq!(
            keyword("check ndim(n): check_ndim {@path} {n}"),
            Some(Keyword::Check)
        );
        assert_eq!(keyword("check = f(x)"), Some(Keyword::Check));
        assert_eq!(keyword("defaults = f(x)"), None);
        assert_eq!(keyword("sources:"), None);
        assert_eq!(
            Header::of("contexts visits:"),
            Some(Header::Contexts(Some("visits")))
        );
        assert_eq!(Header::of("products:"), None);
        assert_eq!(Header::of("contexts"), None);
        assert_eq!(Header::of("sources:"), Some(Header::Sources));
    }
}
