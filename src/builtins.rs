//! SPIT's own words, such as `source`, `@ vary(...)` and `{@entities}`,
//! where a document writes them, for an editor to explain on hover. Each
//! word's documentation is one entry of a static table: an example, a
//! summary of the language reference, and the section it comes from. A
//! document is scanned once, line by line, into small records that name
//! their entry, so the text is written once however often a word is used.
//! Names a document defines are explained by `editor`.

use std::ops::Range;

use crate::json::Json;
use crate::parser::{strip_comment, without_bom, Header, Keyword};
use crate::span::utf16_columns;

/// The language reference the entries summarise; each links to a section.
pub const REFERENCE: &str = "https://github.com/eclnz/spit/blob/main/docs/language-reference.md";

/// One of SPIT's own words, as an index into [`DOCS`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum Word {
    Source,
    Operation,
    Command,
    Verify,
    Path,
    Ext,
    Stage,
    Use,
    UseAs,
    UseFrom,
    Dimensions,
    Sidecars,
    Many,
    Beside,
    Vary,
    Each,
    Where,
    Same,
    Min,
    Pipeline,
    Root,
    Discover,
    DiscoverFrom,
    Require,
    RequireWhere,
    Count,
    Drop,
    DropWhere,
    Missing,
    Has,
    RequireHas,
    Exclude,
    ExcludeFrom,
    Product,
    Entities,
    StageName,
    Labels,
    Output,
    Dir,
    Stem,
    Sources,
    SourcePaths,
    Contexts,
    Removed,
    Check,
    CheckClause,
    PathPlaceholder,
}

/// What a word's hover says.
pub struct Doc {
    /// The word's name in `check --json`, unique among words.
    pub name: &'static str,
    /// `keyword`, `selector`, `placeholder` or `header`.
    pub kind: &'static str,
    /// The section of the language reference it summarises.
    pub anchor: &'static str,
    /// SPIT text that uses it.
    pub example: &'static str,
    /// What it does, as plain text.
    pub summary: &'static str,
}

impl Word {
    pub fn doc(self) -> &'static Doc {
        &DOCS[self as usize]
    }
}

/// Every word's documentation, in the order of [`Word`].
pub static DOCS: [Doc; 47] = [
    Doc {
        name: "source",
        kind: "keyword",
        anchor: "products-and-dimensions",
        example: "source image : Image [subject, visit, run]\nsource calibration",
        summary: "Declares a product family, not one file: `image[subject=A,visit=1,run=2]` names one artifact. The type is optional, and an extension after it, as in `source events : Events .tsv [sub]`, completes the source's path rule. A source with no dimensions takes no brackets and names one artifact, which matches every job that takes it. Its files are found by its `path` rule.",
    },
    Doc {
        name: "operation",
        kind: "keyword",
        anchor: "operations-and-commands",
        example: "operation process(image: Image) -> Image\noperation estimate(dwi: DWI) -> (wm: Response, gm: Response)",
        summary: "Declares a step's input ports and outputs, before its first use. A port is `name`, `name: Type`, `name: many` or `name: many Type`. A call fills the ports in order, and SPIT checks each product's type against its port. Several outputs are each named, and an output's type may be followed by the extension the tool gives its file, as in `-> Transform .mat`.",
    },
    Doc {
        name: "command",
        kind: "keyword",
        anchor: "operations-and-commands",
        example: "command process: process_tool --in {image} --out {@output}",
        summary: "The program an operation runs. Each `{port}` is filled in with an artifact's path, and every output must appear, or its `.dir` or `.stem`, except one written `beside` another. Words are split and quoted as in Bash, and every argument is passed literally: `|`, `>` and `$` are not a shell's. The first word must be an executable on `PATH`, or a path to one.",
    },
    Doc {
        name: "verify",
        kind: "keyword",
        anchor: "operations-and-commands",
        example: "verify register: check_same_grid {moving} {reference}",
        summary: "Checks a job's inputs before its command runs. SPIT does not run it: it writes it into the `.spitdag` beside the job's command, and a backend runs it first. If it fails, the job does not run, and neither does any job that depends on it. It may use input ports only, `many` ones included.",
    },
    Doc {
        name: "path",
        kind: "keyword",
        anchor: "paths",
        example: "path: results/{@product}/{@entities}.txt\npath image: input/{subject}/{visit}/{run}.txt",
        summary: "`path:` sets the default rule; without one, outputs go to `out/{@product}/{@entities}`. `path product:` sets one product's rule, and for a source, how its files are found. A `path:` line in a stage is the default for that stage's products, and one in a `.spitin` recipe the default for sources with no rule. Paths are relative to the dataset root. Text in `[...]` is kept only for a product with a value for every placeholder in it.",
    },
    Doc {
        name: "ext",
        kind: "keyword",
        anchor: "extensions",
        example: "path: derivatives/{@product}/{@entities}\next: .nii.gz",
        summary: "The extension for operations that declare none, completing a default path rule. Like `path:`, it may be written at the top level or in a stage. A product's own `path product:` rule never takes it.",
    },
    Doc {
        name: "stage",
        kind: "keyword",
        anchor: "stages",
        example: "stage preprocess:\n    sorted = sort_lines(shard)",
        summary: "Groups the steps of one phase of a pipeline. Indent the stage's lines beneath it; the next line that is not indented ends it. A stage owns the products its steps assign, while operations and commands stay global. Stages nest, and SPIT orders them by the products they read. `{@stage}` in a path is the stage's name, one directory per level.",
    },
    Doc {
        name: "use",
        kind: "keyword",
        anchor: "reuse-definitions",
        example: "use text.spit as text\nuse shard, sort_lines from text.spit as text",
        summary: "Imports operations and source families from another `.spit` file, relative to this one. An operation brings its `command`, and a source its path rule; steps are not imported.",
    },
    Doc {
        name: "use-as",
        kind: "keyword",
        anchor: "reuse-definitions",
        example: "use text.spit as text\nsorted = text::sort_lines(text::shard)",
        summary: "Gives every name a `use` line imports a prefix, as in `text::shard`. Without it, the names come into this file's scope.",
    },
    Doc {
        name: "use-from",
        kind: "keyword",
        anchor: "reuse-definitions",
        example: "use shard, sort_lines from text.spit",
        summary: "Names the file a `use` line imports only the listed definitions from.",
    },
    Doc {
        name: "dimensions",
        kind: "keyword",
        anchor: "dimension-order",
        example: "dimensions [model, config, seed]",
        summary: "Declares the pipeline's dimension order, once at the top level, for a product holding two dimensions that no source orders. It names every dimension once, and each source lists its dimensions in that order. The order sorts a `many` input's artifacts and writes `{@entities}`.",
    },
    Doc {
        name: "sidecars",
        kind: "keyword",
        anchor: "sidecar-files",
        example: "sidecars photo [site, shot]:\n    path: site-{site}/shot-{shot}\n    source raw_photo : Image .raw\n    source photo_json .json",
        summary: "Declares sources whose files share dimensions and a path stem, and differ only by extension. Each indented member is an ordinary source whose path is the stem and its extension. The block's `path:` line gives the stem, or a recipe gives it as `path name:`. `spit inputs` warns where it finds some of a group's files and not the others.",
    },
    Doc {
        name: "many",
        kind: "keyword",
        anchor: "operations-and-commands",
        example: "operation mean(images: many Image) -> Image\naverage = mean(processed @ vary(run))",
        summary: "A port that collects several artifacts into one job. Each call names the dimensions it collects with `@ vary(...)`. Its placeholder becomes one quoted argument per artifact, in the pipeline's dimension order, and must be a whole argument. An operation takes at most one `many` input.",
    },
    Doc {
        name: "beside",
        kind: "keyword",
        anchor: "files-a-tool-writes-beside-another",
        example: "operation strip(t1: Image) -> (brain: Image .nii.gz, mask: Image \"_mask.nii.gz\" beside brain)",
        summary: "An output the tool writes next to another without being told where. Its path is its sibling's, without the sibling's extension, then the suffix: beside `sub-01_brain.nii.gz`, `mask` is `sub-01_brain_mask.nii.gz`. It may be left out of the command, and has no path rule of its own.",
    },
    Doc {
        name: "vary",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "average = mean(processed @ vary(run))",
        summary: "Collects a `many` input over the named dimensions, which leave the output's identity. One `vary(...)` may name several; the collection follows the pipeline's dimension order whatever order it lists them in.",
    },
    Doc {
        name: "each",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "forecast = predict(reading, model @ each(scenario), parameters)",
        summary: "Broadcasts an input over a dimension the driving input lacks: the step runs once for each of the input's values, and its outputs gain that dimension. The reverse of `vary`. Only one input may broadcast a given dimension.",
    },
    Doc {
        name: "where",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "calibrated = calibrate(reading, calibration @ where(revision=2))",
        summary: "Keeps the artifacts with that value and takes the dimension out of matching, so a family with an extra dimension can join a less specific input.",
    },
    Doc {
        name: "same",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "anomaly = compare(calibrated, reference @ same(station))",
        summary: "Matches on the named dimensions alone. The input's other dimensions must then leave exactly one artifact for each job.",
    },
    Doc {
        name: "min",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "operation summarise(days: many Series @ min(2)) -> Summary",
        summary: "Rejects a group whose `many` input has fewer artifacts than the minimum. With `dag --partial`, it counts what is left once incomplete members are removed.",
    },
    Doc {
        name: "pipeline",
        kind: "keyword",
        anchor: "recipes",
        example: "pipeline analysis.spit",
        summary: "The first line of a `.spitin` recipe: the pipeline it serves, relative to the recipe's folder. `spit check`, `spit inputs` and `spit dag` read the pipeline from it.",
    },
    Doc {
        name: "root",
        kind: "keyword",
        anchor: "recipes",
        example: "root data",
        summary: "The dataset root, the folder paths are relative to. Every recipe names one, relative to the recipe's folder, and `root .` is that folder; in a `.spitout` that `spit inputs -o` writes, it is relative to the `.spitout`'s folder.",
    },
    Doc {
        name: "discover",
        kind: "keyword",
        anchor: "discover-contexts-from-directories",
        example: "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}",
        summary: "Finds a dataset's contexts from its directories: each matching directory, even an empty one, gives one binding, and only those found on disk are used. `sessions` names the rule, which `require` and `drop` can count. A source whose dimensions fit within the rule's expects a file for each binding.",
    },
    Doc {
        name: "discover-from",
        kind: "keyword",
        anchor: "discover-contexts-from-directories",
        example: "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}",
        summary: "The directory pattern a `discover` rule matches, relative to the dataset root. It uses every declared dimension and no other placeholder.",
    },
    Doc {
        name: "require",
        kind: "keyword",
        anchor: "constraints",
        example: "require [subject, visit] where image count>=2\nrequire [subject, visit] where image has run=1,2",
        summary: "Stops the run if any group fails, checked against what `exclude` and `drop` leave. A rule counts artifacts or requires particular values in each group. A rule that finds no group at all is an error.",
    },
    Doc {
        name: "require-where",
        kind: "keyword",
        anchor: "constraints",
        example: "require [subject, visit] where image count=1",
        summary: "Introduces a `require` rule's condition, after the groups it checks: the source or discovery rule to count, then a `count`, `has` values, or both. Each group is checked on its own.",
    },
    Doc {
        name: "count",
        kind: "keyword",
        anchor: "constraints",
        example: "require [subject, visit] where image count>=2\ndrop [sub] where sessions count<2",
        summary: "How many artifacts of a source, or contexts of a discovery, each group holds, compared with `=`, `!=`, `>=`, `<=`, `>` or `<`.",
    },
    Doc {
        name: "drop",
        kind: "keyword",
        anchor: "drop-groups-that-fail-a-criterion",
        example: "drop [sub] where sessions count<2\ndrop [sub, ses] where bold missing run=1,2",
        summary: "Removes every group that meets its condition, with every artifact and discovered context in it. Judged after every `exclude` and before every `require`, whatever order the rules are written in. A rule that would remove every group is an error. Each removed group is reported and recorded in the `.spitout`.",
    },
    Doc {
        name: "drop-where",
        kind: "keyword",
        anchor: "drop-groups-that-fail-a-criterion",
        example: "drop [sub, ses] where t1w count=0",
        summary: "Introduces a `drop` rule's condition: the source or discovery rule to count, then a `count`, `missing` values or `has` values.",
    },
    Doc {
        name: "missing",
        kind: "keyword",
        anchor: "drop-groups-that-fail-a-criterion",
        example: "drop [sub, ses] where bold missing run=1,2",
        summary: "Removes each group without one of the values: here, a session without a run 1 or without a run 2.",
    },
    Doc {
        name: "has",
        kind: "keyword",
        anchor: "drop-groups-that-fail-a-criterion",
        example: "drop [sub, ses] where bold has run=3",
        summary: "Removes each group with one of the values: here, a session with a run 3.",
    },
    Doc {
        name: "require-has",
        kind: "keyword",
        anchor: "constraints",
        example: "require [subject, visit] where image has run=1,2",
        summary: "Requires each group to hold every one of the values: here, a run 1 and a run 2 in each visit.",
    },
    Doc {
        name: "exclude",
        kind: "keyword",
        anchor: "exclude-named-artifacts",
        example: "exclude bold[sub=02,ses=02,run=3]    # corrupted\nexclude [sub=07]                     # withdrew consent",
        summary: "Removes artifacts by name, while their files stay where they are. A source with all its dimensions names one artifact; values alone name a group of every source; a source with some dimensions names part of that source. A comment on the line is kept as the reason. Applies before every other rule, and an exclude that matches nothing is an error.",
    },
    Doc {
        name: "exclude-from",
        kind: "keyword",
        anchor: "exclude-named-artifacts",
        example: "exclude from qc/excluded.csv",
        summary: "Reads `exclude` rules from a CSV file, relative to the recipe's folder, one rule per row. The header names the columns: `product` and `reason` are optional, and every other column is a dimension. An empty cell leaves its column out.",
    },
    Doc {
        name: "@product",
        kind: "placeholder",
        anchor: "paths",
        example: "path: results/{@product}/{@entities}.txt",
        summary: "The product's name in a path, as `aligned`. An imported `alias::name` becomes `alias.name`.",
    },
    Doc {
        name: "@entities",
        kind: "placeholder",
        anchor: "paths",
        example: "path: results/{@product}/{@entities}.txt",
        summary: "Every dimension as `dim=value`, in the pipeline's dimension order, joined by `__`, as `subject=A__run=2`; `global` for a product with no dimensions.",
    },
    Doc {
        name: "@stage",
        kind: "placeholder",
        anchor: "paths",
        example: "path: {@stage}/{@product}/{@entities}.txt",
        summary: "The stage whose block holds the step, one directory per level, as `preprocess/align`. An error for a product made outside every stage, unless it is in an optional `[...]` group.",
    },
    Doc {
        name: "@labels",
        kind: "placeholder",
        anchor: "paths",
        example: "path: derivatives/sub-{sub}[/ses-{ses}]/{@labels}_{@product}",
        summary: "Every dimension as `key-value`, in the pipeline's dimension order, joined by `_`, as `subject-A_run-2`. SPIT warns if a value contains `-`, since a BIDS reader cannot recover it from the file name.",
    },
    Doc {
        name: "@output",
        kind: "placeholder",
        anchor: "operations-and-commands",
        example: "command process: process_tool --in {image} --out {@output}",
        summary: "The path of the operation's single unnamed output, which the command must use. The `@` marks it as supplied by SPIT, unlike a named port such as `{image}`.",
    },
    Doc {
        name: ".dir",
        kind: "placeholder",
        anchor: "operations-and-commands",
        example: "command convert: dcm2niix -o {image.dir} -f {image.stem} {dicom}",
        summary: "The folder of an output's file, for a tool that takes a folder and a name: `.` for a file at the dataset root. Counts as using the output.",
    },
    Doc {
        name: ".stem",
        kind: "placeholder",
        anchor: "operations-and-commands",
        example: "command convert: dcm2niix -o {image.dir} -f {image.stem} {dicom}",
        summary: "An output's file name without its extension, for a tool that adds the extension itself. Needs the output to declare its extension. Counts as using the output.",
    },
    Doc {
        name: "sources:",
        kind: "header",
        anchor: "inputs",
        example: "sources:\n    image[subject=A,visit=1,run=1]",
        summary: "A `.spitout`'s settled source identities. A record names no file of its own: its source's path rule gives it.",
    },
    Doc {
        name: "source_paths:",
        kind: "header",
        anchor: "inputs",
        example: "source_paths:\n    image: data/sub-{sub}/image.nii.gz",
        summary: "A source path rule a recipe declares, written once in the `.spitout`, so the DAG can use it without the recipe.",
    },
    Doc {
        name: "contexts:",
        kind: "header",
        anchor: "inputs",
        example: "contexts sessions:\n    [sub=01,ses=01]:\n        t1w",
        summary: "Groups named even when one of their inputs is absent. `contexts sessions:` holds the bindings the `discover sessions` rule found, with the source identities under each.",
    },
    Doc {
        name: "removed:",
        kind: "header",
        anchor: "inputs",
        example: "removed:\n    [sub=07]\n        rule: drop [sub] where sessions count<2\n        at: line 6\n        found: 1",
        summary: "What the recipe's `exclude` and `drop` rules removed, each with its rule, line, count found and reason. A record, not a rule: the records above already leave these out, and `dag` copies it into the `.spitdag`.",
    },
    Doc {
        name: "check",
        kind: "keyword",
        anchor: "checks",
        example: "check ndim(n): check_ndim {@path} {n}\ncheck nonempty: test -s {@path}",
        summary: "Declares a test of one artifact, run by a backend: `{@path}` is the artifact, and each `{param}` the text an `@ check(...)` gives it. A nonzero exit fails the job, even when its command succeeded. SPIT does not run it.",
    },
    Doc {
        name: "check-clause",
        kind: "selector",
        anchor: "checks",
        example: "operation denoise(dwi: DWI @ check(ndim(4))) -> DWI @ check(nonempty)",
        summary: "Attaches checks to a port or a source. An input's checks, and its source's, run on each artifact before the job's command; an output's run after it, before the job counts as done.",
    },
    Doc {
        name: "@path",
        kind: "placeholder",
        anchor: "checks",
        example: "check nonempty: test -s {@path}",
        summary: "In a `check`, the path of the artifact being checked. A check's command must use it.",
    },
];

/// One use of a word: its 1-based line and 1-based UTF-16 columns, the end
/// exclusive, as in diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WordUse {
    pub line: usize,
    pub columns: Range<usize>,
    pub word: Word,
}

/// Every word `text` uses, in order. Text in a comment uses none.
pub fn builtin_words(text: &str) -> Vec<WordUse> {
    let bom = usize::from(text.starts_with('\u{feff}'));
    let mut uses = Vec::new();
    let mut found = Vec::new();
    for (index, line) in without_bom(text).lines().enumerate() {
        found.clear();
        line_words(line, &mut found);
        found.sort_unstable_by_key(|(bytes, _)| bytes.start);
        let shift = 1 + if index == 0 { bom } else { 0 };
        uses.extend(found.iter().map(|(bytes, word)| {
            let units = utf16_columns(line, bytes);
            WordUse {
                line: index + 1,
                columns: units.start + shift..units.end + shift,
                word: *word,
            }
        }));
    }
    uses
}

/// `check --json`'s `words`, each use naming its word, and `word_docs`,
/// each word used given once.
pub(crate) fn words_json(uses: &[WordUse]) -> [(&'static str, Json<'static>); 2] {
    let mut used = [false; DOCS.len()];
    for word_use in uses {
        used[word_use.word as usize] = true;
    }
    let occurrences = Json::array(uses.iter().map(|word_use| {
        Json::object([
            ("line", Json::Number(word_use.line)),
            ("column", Json::Number(word_use.columns.start)),
            ("end_column", Json::Number(word_use.columns.end)),
            ("word", Json::string(word_use.word.doc().name)),
        ])
    }));
    let docs = Json::object(
        DOCS.iter()
            .zip(used)
            .filter(|(_, used)| *used)
            .map(|(doc, _)| {
                (
                    doc.name,
                    Json::object([
                        ("kind", Json::string(doc.kind)),
                        ("example", Json::string(doc.example)),
                        ("summary", Json::string(doc.summary)),
                        (
                            "reference",
                            Json::String(format!("{REFERENCE}#{}", doc.anchor).into()),
                        ),
                    ]),
                )
            }),
    );
    [("words", occurrences), ("word_docs", docs)]
}

/// The statement a trimmed line starts with, as SPIT's parser classifies it,
/// and the length of its keyword.
fn statement(line: &str) -> Option<(Word, usize)> {
    if let Some((keyword, _)) = Keyword::split(line) {
        let word = match keyword {
            Keyword::Use => Word::Use,
            Keyword::Source => Word::Source,
            Keyword::Discover => Word::Discover,
            Keyword::Operation => Word::Operation,
            Keyword::Command => Word::Command,
            Keyword::Verify => Word::Verify,
            Keyword::Check => Word::Check,
            Keyword::Require => Word::Require,
            Keyword::Drop => Word::Drop,
            Keyword::Exclude => Word::Exclude,
            Keyword::Path => Word::Path,
            Keyword::Ext => Word::Ext,
            Keyword::Stage => Word::Stage,
            Keyword::Dimensions => Word::Dimensions,
            Keyword::Sidecars => Word::Sidecars,
            Keyword::Skip | Keyword::ShellSource => return None,
        };
        let length = if word == Word::Ext {
            3
        } else {
            word.doc().name.len()
        };
        return Some((word, length));
    }
    // A recipe's or `.spitout`'s header lines, as `inputs` reads them.
    [(Word::Pipeline, "pipeline "), (Word::Root, "root ")]
        .into_iter()
        .find(|(_, prefix)| line.starts_with(prefix) && !line.contains('='))
        .map(|(word, prefix)| (word, prefix.len() - 1))
}

/// Each word `line` uses, by byte range.
fn line_words(line: &str, found: &mut Vec<(Range<usize>, Word)>) {
    let code = strip_comment(line);
    let indent = code.len() - code.trim_start().len();
    let trimmed = code.trim();
    if let Some(header) = Header::of(trimmed) {
        let (word, length) = match header {
            Header::Sources => (Word::Sources, "sources".len()),
            Header::SourcePaths => (Word::SourcePaths, "source_paths".len()),
            Header::Contexts(_) => (Word::Contexts, "contexts".len()),
            Header::Removed => (Word::Removed, "removed".len()),
        };
        found.push((indent..indent + length, word));
        return;
    }
    let statement = statement(trimmed);
    let kind = statement.map(|(word, _)| word);
    if let Some((word, length)) = statement {
        found.push((indent..indent + length, word));
    }
    placeholders(code, kind, found);
    let bytes = code.as_bytes();
    let mut start = indent + statement.map_or(0, |(_, length)| length);
    while start < bytes.len() {
        if !(bytes[start].is_ascii_alphabetic() || bytes[start] == b'_') {
            start += 1;
            continue;
        }
        let end = bytes[start..]
            .iter()
            .position(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
            .map_or(bytes.len(), |length| start + length);
        // A word continues a name across `.`, `-` or `::`, as in a file
        // name or an imported product, so only a whole word counts.
        let whole = start == 0 || !matches!(bytes[start - 1], b'.' | b'-' | b':' | b'/');
        if whole {
            if let Some(word) = rule_word(code, start..end, kind) {
                found.push((start..end, word));
            }
        }
        start = end;
    }
}

/// The word `code[range]` is in a line that starts with `statement`, if one.
fn rule_word(code: &str, range: Range<usize>, statement: Option<Word>) -> Option<Word> {
    let text = &code[range.clone()];
    let before = code[..range.start].trim_end();
    let spaced = code[range.end..].starts_with(' ');
    let after = code[range.end..].trim_start();
    let selector = match text {
        "vary" => Some(Word::Vary),
        "each" => Some(Word::Each),
        "where" => Some(Word::Where),
        "same" => Some(Word::Same),
        "min" => Some(Word::Min),
        "check" => Some(Word::CheckClause),
        _ => None,
    };
    if let Some(word) = selector {
        if before.ends_with('@') && after.starts_with('(') {
            return Some(word);
        }
    }
    match (statement?, text) {
        (Word::Operation, "many") if before.ends_with(':') => Some(Word::Many),
        (Word::Operation, "beside") if before.contains("->") => Some(Word::Beside),
        (Word::Use, "as") if spaced => Some(Word::UseAs),
        (Word::Use, "from") if spaced => Some(Word::UseFrom),
        (Word::Discover, "from") if after.starts_with("dirs ") => Some(Word::DiscoverFrom),
        (Word::Discover, "dirs") if before.ends_with(" from") => Some(Word::DiscoverFrom),
        (Word::Exclude, "from") if spaced && before.trim_start() == "exclude" => {
            Some(Word::ExcludeFrom)
        }
        (Word::Require | Word::Drop, "count") if after.starts_with(['=', '!', '>', '<']) => {
            Some(Word::Count)
        }
        (Word::Require, "where") if before.ends_with(']') => Some(Word::RequireWhere),
        (Word::Require, "has") if before.contains(" where ") => Some(Word::RequireHas),
        (Word::Drop, "where") if before.ends_with(']') => Some(Word::DropWhere),
        (Word::Drop, "missing") if before.contains(" where ") => Some(Word::Missing),
        (Word::Drop, "has") if before.contains(" where ") => Some(Word::Has),
        _ => None,
    }
}

/// The built-in placeholders in `code`: SPIT's own `{@...}` anywhere, and
/// in a command, `{@output}` and an output's `.dir` and `.stem`. `{{` and
/// `}}` are literal braces.
fn placeholders(code: &str, statement: Option<Word>, found: &mut Vec<(Range<usize>, Word)>) {
    let in_command = matches!(statement, Some(Word::Command | Word::Verify));
    let in_check = statement == Some(Word::Check);
    let bytes = code.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"{{") || bytes[index..].starts_with(b"}}") {
            index += 2;
            continue;
        }
        if bytes[index] != b'{' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let Some(length) = bytes[start..]
            .iter()
            .position(|byte| matches!(byte, b'{' | b'}'))
        else {
            return;
        };
        let end = start + length;
        if bytes[end] == b'{' {
            index = end;
            continue;
        }
        let name = &code[start..end];
        let word = match name {
            "@product" => Some(Word::Product),
            "@entities" => Some(Word::Entities),
            "@stage" => Some(Word::StageName),
            "@labels" => Some(Word::Labels),
            "@output" if in_command => Some(Word::Output),
            "@path" if in_check => Some(Word::PathPlaceholder),
            _ => None,
        };
        if let Some(word) = word {
            found.push((start..end, word));
        } else if in_command {
            for (suffix, word) in [(".dir", Word::Dir), (".stem", Word::Stem)] {
                if name.len() > suffix.len() && name.ends_with(suffix) {
                    found.push((end - suffix.len()..end, word));
                }
            }
        }
        index = end + 1;
    }
}
