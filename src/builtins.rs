//! SPIT's own words, such as `source`, `@vary(...)` and `{@entities}`,
//! where a document writes them, for an editor to explain on hover. Each
//! word's documentation is one entry of a static table: an example, a
//! summary of the language reference, and the section it comes from. A
//! document is scanned once, line by line, into small records that name
//! their entry, so the text is written once however often a word is used.
//! Names a document defines are explained by `editor`.

use std::ops::Range;

use crate::json::Json;
use crate::parser::{without_bom, Block, Header, Keyword, Layout, Line};
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
    Many,
    Beside,
    Vary,
    Each,
    Where,
    Same,
    Min,
    Pipeline,
    Root,
    Inputs,
    ContextEntry,
    Require,
    RequireWhere,
    Count,
    ExcludeWhere,
    Missing,
    Has,
    RequireHas,
    Exclude,
    ExcludeFrom,
    Product,
    Entities,
    StageName,
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
    ShapeDigits,
    ShapeYear,
    ShapeDate,
    EntitiesFormat,
    Default,
    Types,
}

/// What a word's hover says.
pub struct Doc {
    /// The word's name in `check --json`, unique among words.
    pub name: &'static str,
    /// `keyword`, `selector`, `placeholder`, `shape` or `header`.
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
pub static DOCS: [Doc; 50] = [
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
        summary: "Declares a step's input ports and outputs, before its first use. A port is `name`, `name: Type`, `name: many` or `name: many Type`. A call fills the ports in order, and SPIT checks each product's type against its port. Several outputs are each named, and an output's type may be followed by the extension the tool gives its file, as in `-> Transform .mat`. A header ending in `:` is followed by indented steps that carry the operation out in place of a command.",
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
        example: "path image: input/{subject}/{visit}/{run}.txt\ndefault path: results/{@product}/{@entities}.txt",
        summary: "`path product:` sets one product's rule, and for a source, how its files are found. `default path:` sets the rule for products with none of their own. An output with no rule is an error. A default in a stage is the rule for that stage's products. Recipes give source paths and a fallback inside `inputs:`. Paths are relative to the dataset root. Text in `[...]` is kept only for a product with a value for every placeholder in it.",
    },
    Doc {
        name: "ext",
        kind: "keyword",
        anchor: "extensions",
        example: "default path: derivatives/{@product}/{@entities}\ndefault ext: .nii.gz",
        summary: "`default ext:` is the extension for operations that declare none, completing a default path rule. Like `default path:`, it may be written at the top level or in a stage. A product's own `path product:` rule never takes it.",
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
        name: "many",
        kind: "keyword",
        anchor: "operations-and-commands",
        example: "operation mean(images: many Image) -> Image\naverage = mean(processed @vary(run))",
        summary: "A port that collects several artifacts into one job. Each call names the dimensions it collects with `@vary(...)`. Its placeholder becomes one quoted argument per artifact, in the pipeline's dimension order, and must be a whole argument. An operation takes at most one `many` input.",
    },
    Doc {
        name: "beside",
        kind: "keyword",
        anchor: "sidecar-files",
        example: "source image .nii.gz [sub]\nsource metadata .json beside image\noperation strip(t1: Image) -> (brain: Image .nii.gz, mask: Image \"_mask.nii.gz\" beside brain)",
        summary: "A source or output whose file shares another file's stem. The suffix replaces the other file's extension: beside `sub-01_brain.nii.gz`, a `.json` companion is `sub-01_brain.json`. A source inherits the other's dimensions and path; an output is written by the same job and may be omitted from its command.",
    },
    Doc {
        name: "vary",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "average = mean(processed @vary(run))",
        summary: "Collects a `many` input over the named dimensions, which leave the output's identity. One `vary(...)` may name several; the collection follows the pipeline's dimension order whatever order it lists them in.",
    },
    Doc {
        name: "each",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "forecast = predict(reading, model @each(scenario), parameters)",
        summary: "Broadcasts an input over a dimension the driving input lacks: the step runs once for each of the input's values, and its outputs gain that dimension. The reverse of `vary`. Only one input may broadcast a given dimension.",
    },
    Doc {
        name: "where",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "calibrated = calibrate(reading, calibration @where(revision=2))",
        summary: "Keeps the artifacts with that value and takes the dimension out of matching, so a family with an extra dimension can join a less specific input.",
    },
    Doc {
        name: "same",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "anomaly = compare(calibrated, reference @same(station))",
        summary: "Matches on the named dimensions alone. The input's other dimensions must then leave exactly one artifact for each job.",
    },
    Doc {
        name: "min",
        kind: "selector",
        anchor: "operations-and-commands",
        example: "operation summarise(days: many Series @min(2)) -> Summary",
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
        summary: "The dataset root, the folder paths are relative to. A pipeline or recipe may name one, relative to that file's folder; a recipe inherits its pipeline's root when it has none. Declaring it in both is an error, and `root .` is the declaring file's folder; in a `.spitout` that `spit inputs -o` writes, it is relative to the `.spitout`'s folder.",
    },
    Doc {
        name: "inputs:",
        kind: "keyword",
        anchor: "recipes",
        example: "inputs:\n    image: data/sub-{sub}/image.nii.gz\n    contexts: data/sub-{sub}",
        summary: "A recipe's source paths and optional context directory patterns. Each source entry matches existing files; each `contexts:` entry observes matching directories, including empty ones. Neither creates a requirement by itself. `default:` is the fallback source path.",
    },
    Doc {
        name: "contexts",
        kind: "keyword",
        anchor: "recipes",
        example: "    contexts: data/sub-{sub}/ses-{ses}",
        summary: "A directory pattern in `inputs:`. Matching directories contribute observed bindings; placeholders supply dimensions. It may match no directories and does not require source files.",
    },
    Doc {
        name: "require",
        kind: "keyword",
        anchor: "constraints",
        example: "require [subject, visit] where image count>=2\nrequire [subject, visit] where image has run=1,2",
        summary: "Stops the run if any group fails, checked against what named and conditional `exclude` leave. A rule counts artifacts or requires particular values in each group. A rule that finds no group at all is an error.",
    },
    Doc {
        name: "require-where",
        kind: "keyword",
        anchor: "constraints",
        example: "require [subject, visit] where image count=1",
        summary: "Introduces a `require` rule's condition, after the groups it checks: the source or `contexts[dimensions]` target to count, then a `count`, `has` values, or both. Each group is checked on its own.",
    },
    Doc {
        name: "count",
        kind: "keyword",
        anchor: "constraints",
        example: "require [subject, visit] where image count>=2\nexclude [sub] where sessions count<2",
        summary: "How many artifacts of a source, or contexts in an exact dimension domain, each group holds, compared with `=`, `!=`, `>=`, `<=`, `>` or `<`.",
    },
    Doc {
        name: "exclude-where",
        kind: "keyword",
        anchor: "exclude-groups-that-meet-a-condition",
        example: "exclude [sub, ses] where t1w count=0",
        summary: "Introduces a conditional `exclude` rule's condition: the source or `contexts[dimensions]` target to count, then a `count`, `missing` values or `has` values.",
    },
    Doc {
        name: "missing",
        kind: "keyword",
        anchor: "exclude-groups-that-meet-a-condition",
        example: "exclude [sub, ses] where bold missing run=1,2",
        summary: "Removes each group without one of the values: here, a session without a run 1 or without a run 2.",
    },
    Doc {
        name: "has",
        kind: "keyword",
        anchor: "exclude-groups-that-meet-a-condition",
        example: "exclude [sub, ses] where bold has run=3",
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
        example: "exclude bold[sub=02,ses=02,run=3]    # corrupted\nexclude [sub] where sessions count<2",
        summary: "Removes named artifacts or groups and groups meeting a condition. Named exclusions apply before scanning; conditional exclusions run against the observed inventory before `require`. An exclude that names nothing or removes every group is an error.",
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
        example: "default entities: {@key}={@value} separated \"__\"\ndefault path: results/{@product}/{@entities}.txt",
        summary: "The product's name in a path, as `aligned`. An imported `alias::name` becomes `alias.name`.",
    },
    Doc {
        name: "@entities",
        kind: "placeholder",
        anchor: "paths",
        example: "default path: results/{@product}/{@entities}.txt",
        summary: "Every dimension, in the pipeline's dimension order, as the nearest `default entities:` spells it: the product's stage's, else the file's. Values and labels are percent escaped. A product with no dimensions has no `{@entities}`, so a default that covers one puts it in `[...]`.",
    },
    Doc {
        name: "@stage",
        kind: "placeholder",
        anchor: "paths",
        example: "default path: {@stage}/{@product}/{@entities}.txt",
        summary: "The stage whose block holds the step, one directory per level, as `preprocess/align`. An error for a product made outside every stage, unless it is in an optional `[...]` group.",
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
        example: "contexts:\n    [sub=01,ses=01]",
        summary: "Observed directory bindings, including empty directories. Context paths in a recipe are unnamed; their capture dimensions determine which `contexts[dimensions]` requirement counts them.",
    },
    Doc {
        name: "removed:",
        kind: "header",
        anchor: "inputs",
        example: "removed:\n    [sub=07]\n        rule: exclude [sub] where sessions count<2\n        at: line 6\n        found: 1",
        summary: "What the recipe's `exclude` rules removed, each with its rule, line, count found and reason. A record, not a rule: the records above already leave these out, and `dag` copies it into the `.spitdag`.",
    },
    Doc {
        name: "check",
        kind: "keyword",
        anchor: "checks",
        example: "check ndim(n): check_ndim {@path} {n}\ncheck nonempty: test -s {@path}\ndefault check: nonempty",
        summary: "Declares a test of one artifact, run by a backend: `{@path}` is the artifact, and each `{param}` the text an `@check(...)` gives it. A nonzero exit fails the job, even when its command succeeded. SPIT does not run it. `default check:` lists the checks every output in the file, or in the stage it is written in, runs; `!name` drops one a wider scope sets.",
    },
    Doc {
        name: "check-clause",
        kind: "selector",
        anchor: "checks",
        example: "operation denoise(dwi: DWI @check(ndim(4))) -> DWI @check(nonempty)",
        summary: "Attaches checks to a port or a source. An input's checks, and its source's, run on each artifact before the job's command; an output's run after it, before the job counts as done.",
    },
    Doc {
        name: "@path",
        kind: "placeholder",
        anchor: "checks",
        example: "check nonempty: test -s {@path}",
        summary: "In a `check`, the path of the artifact being checked. A check's command must use it.",
    },
    Doc {
        name: "digits",
        kind: "shape",
        anchor: "shapes-on-a-source-placeholder",
        example: "path run: raw/run-{run:digits}.csv",
        summary: "A shape for a placeholder in a source's path rule: one or more digits, as `07` or `120`. A file whose name has anything else there is not read as that source. Leading zeros are kept in the value.",
    },
    Doc {
        name: "year",
        kind: "shape",
        anchor: "shapes-on-a-source-placeholder",
        example: "path report: reports/{year:year}/summary.csv",
        summary: "A shape for a placeholder in a source's path rule: four digits, from 1900 to 2099, as `2026`.",
    },
    Doc {
        name: "date",
        kind: "shape",
        anchor: "shapes-on-a-source-placeholder",
        example: "path log: logs/{server}/{date:date}.log",
        summary: "A shape for a placeholder in a source's path rule: a date written `YYYY-MM-DD`, a real day in a year from 1900 to 2099, as `2026-09-01`. `2026-13-01` and `2026-9-1` are not read.",
    },
    Doc {
        name: "entities",
        kind: "keyword",
        anchor: "entity-formatting",
        example: "default entities: {@key}_{@value} separated \"-\"\ndefault entities sub: subject",
        summary: "After `default`, spells `{@entities}`: `{@key}` is each dimension's label, `{@value}` its escaped value, and `separated` the text between them. `default entities sub: subject` changes that dimension's label in paths; its identities and order stay the same. A spelling covers the products of the file or of the stage it is written in, and one in a `.spitin` recipe the rules the recipe gives."
    },
    Doc {
        name: "default",
        kind: "keyword",
        anchor: "defaults",
        example: "default path: results/{@product}/{@entities}\ndefault:\n    ext: .nii.gz\n    check: nonempty",
        summary: "Sets a default for the outputs of the file, or of the stage it is written in: `default path:` the rule for products with none of their own, `default ext:` the extension that completes it, `default check:` the checks every output runs, and `default entities:` how `{@entities}` is spelled. `default:` opens a block of them. In a `.spitin` recipe, `default path:` is the rule for sources with none of their own.",
    },
    Doc {
        name: "types",
        kind: "keyword",
        anchor: "optional-types",
        example: "types strict",
        summary: "How much of the pipeline must be typed, once at the top level. `types strict` makes it an error for a source to have no type, for an operation a step or call uses to have a port with no type, or for a product a step makes to have a type inference leaves partly unknown. `types standard`, the default, checks only the types that are known. An imported file's `types` line does not apply.",
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
    let layout = Layout::new(without_bom(text));
    // For each line that a statement's keyword covers without being on it,
    // that keyword's word: an entry of a keyword block, such as `source:`,
    // and each line after a statement's first, such as a command's.
    let mut inherited = vec![None; layout.lines.len()];
    for statement in &layout.statements {
        let block = statement
            .parent
            .and_then(|parent| match layout.statements[parent].opens {
                Some(Block::Keyword(keyword)) => keyword_word(keyword),
                _ => None,
            });
        let first = statement.lines.start;
        inherited[first] = block;
        let own = layout.lines[first].keyword().and_then(keyword_word);
        inherited[first + 1..statement.lines.end].fill(own.or(block));
    }
    for (index, read) in layout.lines.iter().enumerate() {
        let line = read.text;
        found.clear();
        line_words(read, inherited[index], &mut found);
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

/// The word of a statement that starts with `keyword`, if SPIT documents
/// one.
fn keyword_word(keyword: Keyword) -> Option<Word> {
    Some(match keyword {
        Keyword::Use => Word::Use,
        Keyword::Root => Word::Root,
        Keyword::Source => Word::Source,
        Keyword::Operation => Word::Operation,
        Keyword::Command => Word::Command,
        Keyword::Verify => Word::Verify,
        Keyword::Check | Keyword::Checks => Word::Check,
        Keyword::Require => Word::Require,
        Keyword::Exclude => Word::Exclude,
        Keyword::Path => Word::Path,
        Keyword::Ext => Word::Ext,
        Keyword::Default => Word::Default,
        Keyword::Stage => Word::Stage,
        Keyword::Dimensions => Word::Dimensions,
        Keyword::Types => Word::Types,
        Keyword::Entities => Word::EntitiesFormat,
        Keyword::Drop | Keyword::Sidecars | Keyword::Skip | Keyword::ShellSource => return None,
    })
}

/// The statement `line` starts with, as SPIT's parser classifies it,
/// and the length of its keyword.
fn statement(line: &Line) -> Option<(Word, usize)> {
    if let Some(keyword) = line.keyword() {
        let word = keyword_word(keyword)?;
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
        .find(|(_, prefix)| line.code.starts_with(prefix) && !line.code.contains('='))
        .map(|(word, prefix)| (word, prefix.len() - 1))
}

/// Each word `line` uses, by byte range.
/// The words `line` uses, its statement's word being `inherited` when the
/// keyword that gives it is not on the line.
fn line_words(line: &Line, inherited: Option<Word>, found: &mut Vec<(Range<usize>, Word)>) {
    let code = line.uncommented;
    let indent = line.indent;
    let trimmed = code.trim();
    if trimmed == "inputs:" {
        found.push((indent..indent + "inputs:".len(), Word::Inputs));
        return;
    }
    if indent > 0 && trimmed.starts_with("contexts: ") {
        found.push((indent..indent + "contexts".len(), Word::ContextEntry));
    }
    if let Some(header) = line.header() {
        let (word, length) = match header {
            Header::Sources => (Word::Sources, "sources".len()),
            Header::SourcePaths => (Word::SourcePaths, "source_paths".len()),
            Header::Contexts(_) => (Word::Contexts, "contexts".len()),
            Header::Removed => (Word::Removed, "removed".len()),
        };
        found.push((indent..indent + length, word));
        return;
    }
    // The statement's word, the offset it starts at and its length. After
    // `default`, the setting is read as its own line would be.
    let mut statement = statement(line).map(|(word, length)| (word, indent, length));
    if let Some((Word::Default, at, length)) = statement {
        found.push((at..at + length, Word::Default));
        let rest = &code[at + length..];
        let setting = code.len() - rest.trim_start().len();
        statement = self::statement(&Line::new(&code[setting..]))
            .map(|(word, length)| (word, setting, length));
    }
    if let Some((word, at, length)) = statement {
        found.push((at..at + length, word));
    }
    let kind = statement.map(|(word, _, _)| word).or(inherited);
    placeholders(code, kind, found);
    let bytes = code.as_bytes();
    let mut start = statement.map_or(indent, |(_, at, length)| at + length);
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
    let tight = code[..range.start].ends_with('@');
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
        // Keep in step with `no_space_after_at` in `parser/check.rs`: a clause
        // is `@keyword(`, with nothing between the `@` and the keyword.
        if tight && after.starts_with('(') {
            return Some(word);
        }
    }
    match (statement?, text) {
        (Word::Operation, "many") if before.ends_with(':') => Some(Word::Many),
        (Word::Operation, "beside") if before.contains("->") => Some(Word::Beside),
        (Word::Source, "beside") if before.split_whitespace().count() > 1 && spaced => {
            Some(Word::Beside)
        }
        (Word::Use, "as") if spaced => Some(Word::UseAs),
        (Word::Use, "from") if spaced => Some(Word::UseFrom),
        (Word::Exclude, "from") if spaced && before.trim_start() == "exclude" => {
            Some(Word::ExcludeFrom)
        }
        (Word::Require | Word::Exclude, "count") if after.starts_with(['=', '!', '>', '<']) => {
            Some(Word::Count)
        }
        (Word::Require, "where") if before.ends_with(']') => Some(Word::RequireWhere),
        (Word::Require, "has") if before.contains(" where ") => Some(Word::RequireHas),
        (Word::Exclude, "where") if before.ends_with(']') => Some(Word::ExcludeWhere),
        (Word::Exclude, "missing") if before.contains(" where ") => Some(Word::Missing),
        (Word::Exclude, "has") if before.contains(" where ") => Some(Word::Has),
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
            "@output" if in_command => Some(Word::Output),
            "@path" if in_check => Some(Word::PathPlaceholder),
            _ => None,
        };
        if let Some(word) = word {
            found.push((start..end, word));
        } else if !in_command && !in_check {
            // A shape after the `:` of a dimension, as in `{date:date}`.
            if let Some((dimension, shape)) = name.split_once(':') {
                let word = match shape {
                    "digits" => Some(Word::ShapeDigits),
                    "year" => Some(Word::ShapeYear),
                    "date" => Some(Word::ShapeDate),
                    _ => None,
                };
                if let (Some(word), false) = (word, dimension.starts_with('@')) {
                    found.push((end - shape.len()..end, word));
                }
            }
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
