//! What a line starts with: a keyword such as `source` or `path`, or a
//! records header such as `sources:` or `contexts name:`. Every reader of
//! a document classifies its lines here, so they agree on what each is.

/// A keyword that starts a statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Keyword {
    Use,
    Source,
    Discover,
    Operation,
    Command,
    Verify,
    /// `check name(params): command`, a test of one artifact.
    Check,
    /// `check:`, the checks every output in a file or stage runs.
    Checks,
    Require,
    /// `skip` and `drop`, kept to give migration errors.
    Skip,
    Drop,
    Exclude,
    /// `path:` for a default, `path product:` for one product.
    Path,
    /// `ext:`, the extension a default path is completed with.
    Ext,
    Entities,
    Stage,
    /// `dimensions [...]`, the order every product's dimensions follow.
    Dimensions,
    /// The removed `sidecars` block, kept to explain its replacement.
    Sidecars,
    /// The removed `shell-source:` line, kept to explain its removal.
    ShellSource,
}

const WORDS: [(Keyword, &str); 14] = [
    (Keyword::Use, "use"),
    (Keyword::Source, "source"),
    (Keyword::Discover, "discover"),
    (Keyword::Operation, "operation"),
    (Keyword::Command, "command"),
    (Keyword::Verify, "verify"),
    (Keyword::Check, "check"),
    (Keyword::Require, "require"),
    (Keyword::Skip, "skip"),
    (Keyword::Drop, "drop"),
    (Keyword::Exclude, "exclude"),
    (Keyword::Stage, "stage"),
    (Keyword::Dimensions, "dimensions"),
    (Keyword::Sidecars, "sidecars"),
];

impl Keyword {
    /// The keyword `line` starts with, and the text after it. A trimmed
    /// line is expected. `stage` and `dimensions` start a statement only
    /// without `=`, since a step's output product may have either name.
    pub(crate) fn split(line: &str) -> Option<(Self, &str)> {
        if let Some(rest) = line.strip_prefix("shell-source:") {
            return Some((Self::ShellSource, rest));
        }
        if let Some(rest) = line.strip_prefix("path") {
            if rest.starts_with([' ', ':']) {
                return Some((Self::Path, rest));
            }
        }
        if let Some(rest) = line.strip_prefix("entities:") {
            if rest.contains("{key}") || rest.contains("{value}") || !has_top_level_equals(rest) {
                return Some((Self::Entities, rest));
            }
        }
        if let Some(rest) = line.strip_prefix("entities ") {
            if rest
                .trim_start()
                .starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && !rest.split(':').next().unwrap_or_default().contains('=')
            {
                return Some((Self::Entities, rest));
            }
        }
        // A step may annotate an output named `ext`: `ext: Image = ...`.
        if let Some(rest) = line.strip_prefix("ext:") {
            return (!line.contains('=')).then_some((Self::Ext, rest));
        }
        // A step may make a product named `check`: `check: Report = f(x)`.
        // An `=` inside parentheses is a check's argument, not a step.
        if let Some(rest) = line.strip_prefix("check:") {
            return (!has_top_level_equals(rest)).then_some((Self::Checks, rest));
        }
        let (word, rest) = line.split_once(' ')?;
        let (keyword, _) = WORDS.iter().find(|(_, name)| *name == word)?;
        if matches!(keyword, Self::Stage | Self::Dimensions | Self::Sidecars) && line.contains('=')
        {
            return None;
        }
        // A step may make a product named `check`, as in `check = f(x)` or
        // `check : Report = f(x)`; a declaration names the check before any
        // `:` or `=`.
        if *keyword == Self::Check {
            let head = rest.split(':').next().unwrap_or_default();
            let named = rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
            if !named || head.contains('=') {
                return None;
            }
        }
        Some((*keyword, rest))
    }

    pub(crate) fn of(line: &str) -> Option<Self> {
        Self::split(line).map(|(keyword, _)| keyword)
    }
}

/// Whether `text` has an `=` outside parentheses.
fn has_top_level_equals(text: &str) -> bool {
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

    #[test]
    fn lines_are_classified_by_their_first_word() {
        assert_eq!(
            Keyword::split("source raw [id]"),
            Some((Keyword::Source, "raw [id]"))
        );
        assert_eq!(Keyword::of("path: out/{@entities}"), Some(Keyword::Path));
        assert_eq!(Keyword::of("path raw: in/{id}"), Some(Keyword::Path));
        assert_eq!(Keyword::of("paths = f(x)"), None);
        assert_eq!(Keyword::of("stage analysis:"), Some(Keyword::Stage));
        assert_eq!(Keyword::of("stage = f(x)"), None);
        assert_eq!(
            Keyword::of("check ndim(n): check_ndim {@path} {n}"),
            Some(Keyword::Check)
        );
        assert_eq!(Keyword::of("check = f(x)"), None);
        assert_eq!(Keyword::of("check : Report = f(x)"), None);
        assert_eq!(Keyword::of("sources:"), None);
        assert_eq!(
            Header::of("contexts visits:"),
            Some(Header::Contexts(Some("visits")))
        );
        assert_eq!(Header::of("products:"), None);
        assert_eq!(Header::of("contexts"), None);
        assert_eq!(Header::of("sources:"), Some(Header::Sources));
    }
}
