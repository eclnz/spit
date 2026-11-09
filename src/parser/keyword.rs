//! What a line starts with: a keyword such as `source` or `path`, or a
//! records header such as `sources:` or `contexts name:`. Every reader of
//! a document classifies its lines here, so they agree on what each is.

use crate::json::{write_array, ObjectWriter};

/// A keyword that starts a statement. The variants are declared in the
/// order of [`Keyword::ALL`], which is the order suggestions are tried in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Keyword {
    Source,
    /// `dimensions [...]`, the order every product's dimensions follow.
    Dimensions,
    /// `types strict` or `types standard`, how much of the pipeline must be typed.
    Types,
    Operation,
    Command,
    Verify,
    /// `resources operation: cores=8, memory=16G`, what each job of an
    /// operation needs.
    Resources,
    /// `check name(params): command`, a test of one artifact.
    Check,
    /// `review name`, which declares a review, and in a recipe `review name
    /// from file.csv`, the file its verdicts are in.
    Review,
    /// `path product:` for one product's rule, and `path:` for the default
    /// one, written after `default` or in a `default:` block.
    Path,
    /// `default path:`, `default ext:`, `default check:` and `default review:`, and `default:`,
    /// which opens a block of them.
    Default,
    Stage,
    Use,
    Root,
    /// `output <directory>`, the folder derived files are written under.
    OutputRoot,
    Require,
    Drop,
    Exclude,
    /// `check:`, the checks every output in a file or stage runs, written
    /// after `default` or in a `default:` block.
    Checks,
    /// `review:`, the reviews that judge every output in a file or stage,
    /// written after `default` or in a `default:` block.
    Reviews,
    /// `ext:`, the extension a default path is completed with, written
    /// after `default` or in a `default:` block.
    Ext,
    Entities,
    Skip,
    /// `sidecars`, which starts no statement, read to say what to write instead.
    Sidecars,
    /// `shell-source:`, which starts no statement, read to say what to write instead.
    ShellSource,
    /// `type Name<Param, ...>`, which declares a parameterized type.
    Type,
}

/// Which file a keyword's statement belongs in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum File {
    /// A `.spit` pipeline; a recipe rejects the line as the pipeline's.
    Pipeline,
    /// A `.spitin` recipe.
    Recipe,
    /// Either file.
    Both,
    /// Neither: a word SPIT replaced, read in either file only to say what
    /// to write instead.
    Replaced,
}

/// What SPIT knows of one keyword, from [`Keyword::spec`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Spec {
    /// The word a line starts with. Each is reserved: no product may take
    /// it as its name, so the first word of a line says whether it starts
    /// a statement. `None` for a keyword [`Keyword::split`] tells from
    /// another's word by what follows it.
    pub(crate) word: Option<&'static str>,
    /// Whether the word alone with a `:`, as `source:`, opens a block of
    /// its statements.
    pub(crate) block: bool,
    /// The file its statement belongs in.
    pub(crate) file: File,
    /// Whether an unknown first word close to this one is told to write it.
    pub(crate) suggested: bool,
}

impl Keyword {
    /// Every keyword, in declaration order, which the assertion below keeps.
    pub(crate) const ALL: [Self; 26] = [
        Self::Source,
        Self::Dimensions,
        Self::Types,
        Self::Operation,
        Self::Command,
        Self::Verify,
        Self::Resources,
        Self::Check,
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
        Self::Checks,
        Self::Reviews,
        Self::Ext,
        Self::Entities,
        Self::Skip,
        Self::Sidecars,
        Self::ShellSource,
        Self::Type,
    ];

    /// The one table of keywords: every list of them derives from it.
    pub(crate) const fn spec(self) -> Spec {
        use File::{Both, Pipeline, Recipe, Replaced};
        let (word, block, file, suggested) = match self {
            Self::Source => ("source", true, Pipeline, true),
            Self::Dimensions => ("dimensions", false, Pipeline, true),
            Self::Types => ("types", false, Pipeline, true),
            Self::Operation => ("operation", true, Pipeline, true),
            Self::Command => ("command", true, Pipeline, true),
            Self::Verify => ("verify", true, Pipeline, true),
            Self::Resources => ("resources", true, Pipeline, true),
            Self::Check => ("check", true, Pipeline, true),
            // `review name from file.csv` names a review's file in a
            // recipe; the parser rejects `review name` alone there.
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
            Self::Checks | Self::Reviews => {
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
            // A recipe rejects `sidecars` as the pipeline's, and the
            // pipeline says what to write instead.
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

    /// The words of the keywords [`Spec::suggested`] marks, in the order
    /// of [`Keyword::ALL`].
    pub(crate) const SUGGESTED: [&'static str; count(Filter::Suggested)] = words(Filter::Suggested);
}

/// Which keywords a derived list holds.
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

/// The keywords with a word of their own, and the word of each. The
/// last variant closes [`Keyword::ALL`], so no keyword is left out of it.
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

/// Every keyword with a word of its own, in the order of [`Keyword::ALL`],
/// as one line of JSON for tools that must know SPIT's keywords, such as an
/// editor's grammar: `{"keywords":[{"word":"source","block":true,"file":"pipeline"},...]}`.
/// `block` says whether the word alone with a `:` opens a block, and `file`
/// is `pipeline`, `recipe`, `both`, or `replaced` for a word read only to
/// say what to write instead.
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

/// Whether `name` is a keyword's word, which no product may take.
pub(crate) fn reserved(name: &str) -> bool {
    WORDS.iter().any(|(_, word)| *word == name)
}

impl Keyword {
    /// The keyword `line` starts with, and the text after its word. A
    /// trimmed line is expected. The first word, up to a space or a `:`,
    /// decides, since every keyword's word is reserved; the text after it
    /// starts after one space. `check` followed by
    /// `:` and a list is the default checks list, `Checks`, and `review`
    /// followed by `:` and a list is `Reviews`; `ext:`, `check:` and
    /// `review:` give the text after their `:`. A keyword that opens a
    /// block, alone with its `:`, gives `:` as its text.
    pub(crate) fn split(line: &str) -> Option<(Self, &str)> {
        let end = line.find([' ', '\t', ':']).unwrap_or(line.len());
        let (word, rest) = line.split_at(end);
        let &(keyword, _) = WORDS.iter().find(|(_, name)| *name == word)?;
        if rest == ":" && keyword.spec().block {
            return Some((keyword, ":"));
        }
        Some(match (keyword, rest.strip_prefix(':')) {
            (Self::Check, Some(list)) => (Self::Checks, list),
            (Self::Review, Some(list)) => (Self::Reviews, list),
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
    /// `verdicts:`, the rows of the review files a recipe names.
    Verdicts,
}

impl<'a> Header<'a> {
    /// The header a trimmed `line` is, if it is one.
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
