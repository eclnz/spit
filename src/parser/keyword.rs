use crate::json::{write_array, ObjectWriter};

use super::scan::{self, find_outside};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Keyword {
    Source,
    Dimensions,
    Types,
    Operation,
    Command,
    Calculate,
    Verify,
    Resources,
    Tool,
    Uses,
    Anchored,
    Check,
    Screen,
    Review,
    Path,
    Default,
    Stage,
    Use,
    Root,
    OutputRoot,
    Require,
    Drop,
    Exclude,
    Prefer,
    Tools,
    Checks,
    Screens,
    Reviews,
    Ext,
    Entities,
    Skip,
    Sidecars,
    ShellSource,
    Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum File {
    Pipeline,
    Recipe,
    Both,
    Replaced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Spec {
    pub(crate) word: Option<&'static str>,
    pub(crate) block: bool,
    pub(crate) file: File,
    pub(crate) suggested: bool,
}

impl Keyword {
    pub(crate) const ALL: [Self; 34] = [
        Self::Source,
        Self::Dimensions,
        Self::Types,
        Self::Operation,
        Self::Command,
        Self::Calculate,
        Self::Verify,
        Self::Resources,
        Self::Tool,
        Self::Uses,
        Self::Anchored,
        Self::Check,
        Self::Screen,
        Self::Review,
        Self::Path,
        Self::Default,
        Self::Stage,
        Self::Use,
        Self::Root,
        Self::OutputRoot,
        Self::Require,
        Self::Drop,
        Self::Exclude,
        Self::Prefer,
        Self::Tools,
        Self::Checks,
        Self::Screens,
        Self::Reviews,
        Self::Ext,
        Self::Entities,
        Self::Skip,
        Self::Sidecars,
        Self::ShellSource,
        Self::Type,
    ];

    pub(crate) const fn spec(self) -> Spec {
        use File::{Both, Pipeline, Recipe, Replaced};
        let (word, block, file, suggested) = match self {
            Self::Source => ("source", true, Pipeline, true),
            Self::Dimensions => ("dimensions", false, Pipeline, true),
            Self::Types => ("types", false, Pipeline, true),
            Self::Operation => ("operation", true, Pipeline, true),
            Self::Command => ("command", true, Pipeline, true),
            Self::Calculate => ("calculate", false, Pipeline, true),
            Self::Verify => ("verify", true, Pipeline, true),
            Self::Resources => ("resources", true, Pipeline, true),
            Self::Tool => ("tool", true, Pipeline, true),
            Self::Uses => ("uses", true, Pipeline, true),
            Self::Anchored => ("anchored", true, Pipeline, true),
            Self::Check => ("check", true, Pipeline, true),
            Self::Screen => ("screen", true, Pipeline, true),
            Self::Review => ("review", true, Both, true),
            Self::Path => ("path", true, Both, true),
            Self::Default => ("default", false, Both, true),
            Self::Stage => ("stage", false, Pipeline, true),
            Self::Use => ("use", false, Pipeline, true),
            Self::Root => ("root", false, Both, true),
            Self::OutputRoot => ("output", false, Both, true),
            Self::Require => ("require", true, Recipe, true),
            Self::Drop => ("drop", false, Replaced, true),
            Self::Exclude => ("exclude", true, Recipe, true),
            Self::Prefer => ("prefer", true, Recipe, true),
            Self::Tools => ("tools", false, Recipe, true),
            Self::Checks | Self::Screens | Self::Reviews => {
                return Spec {
                    word: None,
                    block: false,
                    file: Pipeline,
                    suggested: false,
                }
            }
            Self::Ext => ("ext", false, Pipeline, false),
            Self::Entities => ("entities", false, Both, false),
            Self::Skip => ("skip", false, Replaced, false),
            Self::Sidecars => ("sidecars", false, Pipeline, false),
            Self::ShellSource => ("shell-source", false, Replaced, false),
            Self::Type => ("type", false, Pipeline, false),
        };
        Spec {
            word: Some(word),
            block,
            file,
            suggested,
        }
    }

    pub(crate) const SUGGESTED: [&'static str; count(Filter::Suggested)] = words(Filter::Suggested);
}

#[derive(Clone, Copy)]
enum Filter {
    Spelled,
    Suggested,
}

const fn holds(filter: Filter, spec: Spec) -> bool {
    match filter {
        Filter::Spelled => spec.word.is_some(),
        Filter::Suggested => spec.suggested,
    }
}

const fn count(filter: Filter) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < Keyword::ALL.len() {
        let keyword = Keyword::ALL[index];
        assert!(
            keyword as usize == index,
            "Keyword::ALL is in declaration order"
        );
        if holds(filter, keyword.spec()) {
            count += 1;
        }
        index += 1;
    }
    count
}

const fn words<const N: usize>(filter: Filter) -> [&'static str; N] {
    let mut words = [""; N];
    let mut filled = 0;
    let mut index = 0;
    while index < Keyword::ALL.len() {
        let spec = Keyword::ALL[index].spec();
        if let (true, Some(word)) = (holds(filter, spec), spec.word) {
            words[filled] = word;
            filled += 1;
        }
        index += 1;
    }
    words
}

const WORDS: [(Keyword, &str); count(Filter::Spelled)] = {
    assert!(Keyword::ALL.len() == Keyword::Type as usize + 1);
    let mut table = [(Keyword::Source, ""); count(Filter::Spelled)];
    let mut filled = 0;
    let mut index = 0;
    while index < Keyword::ALL.len() {
        let keyword = Keyword::ALL[index];
        if let Some(word) = keyword.spec().word {
            table[filled] = (keyword, word);
            filled += 1;
        }
        index += 1;
    }
    table
};

pub fn keywords_json() -> String {
    let mut out = String::new();
    let mut object = ObjectWriter::start(&mut out);
    object.field("keywords", |out| {
        write_array(out, WORDS, |out, (keyword, word)| {
            let spec = keyword.spec();
            let mut entry = ObjectWriter::start(out);
            entry.string("word", word);
            entry.raw("block", if spec.block { "true" } else { "false" });
            entry.string(
                "file",
                match spec.file {
                    File::Pipeline => "pipeline",
                    File::Recipe => "recipe",
                    File::Both => "both",
                    File::Replaced => "replaced",
                },
            );
            entry.finish();
        });
    });
    object.finish();
    out.push('\n');
    out
}

pub(crate) fn reserved(name: &str) -> bool {
    WORDS.iter().any(|(_, word)| *word == name)
}

impl Keyword {
    pub(crate) fn split(line: &str) -> Option<(Self, &str)> {
        let end = line.find([' ', '\t', ':']).unwrap_or(line.len());
        let (word, rest) = line.split_at(end);
        let &(keyword, _) = WORDS.iter().find(|(_, name)| *name == word)?;
        if rest == ":" && keyword.spec().block {
            return Some((keyword, ":"));
        }
        Some(match (keyword, rest.strip_prefix(':')) {
            (Self::Check, Some(list)) => (Self::Checks, list),
            (Self::Screen, Some(list)) => (Self::Screens, list),
            (Self::Review, Some(list)) => (Self::Reviews, list),
            (Self::Ext | Self::ShellSource, Some(after)) => (keyword, after),
            _ => (keyword, rest.strip_prefix(' ').unwrap_or(rest)),
        })
    }
}

pub(super) fn has_top_level_equals(text: &str) -> bool {
    find_outside(text, scan::PARENS, '=').is_some()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Header<'a> {
    Sources,
    SourcePaths,
    Contexts(Option<&'a str>),
    Removed,
    Verdicts,
}

impl<'a> Header<'a> {
    pub(crate) fn of(line: &'a str) -> Option<Self> {
        Some(match line {
            "sources:" => Self::Sources,
            "source_paths:" => Self::SourcePaths,
            "removed:" => Self::Removed,
            "verdicts:" => Self::Verdicts,
            "contexts:" => Self::Contexts(None),
            _ => Self::Contexts(Some(
                line.strip_prefix("contexts ")?.strip_suffix(':')?.trim(),
            )),
        })
    }
}

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
        assert_eq!(keyword("review looked_at"), Some(Keyword::Review));
        assert_eq!(keyword("review: looked_at"), Some(Keyword::Reviews));
        assert_eq!(Keyword::split("review:"), Some((Keyword::Review, ":")));
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
