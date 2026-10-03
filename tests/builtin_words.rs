use spit::{builtin_words, Word, DOCS};
use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};

/// The word at the last character of `text` in `line`, if any.
fn word_at(line: &str, text: &str) -> Option<&'static str> {
    let start = line
        .find(text)
        .unwrap_or_else(|| panic!("`{text}` not in `{line}`"));
    let column = line[..start + text.len() - 1].encode_utf16().count() + 1;
    builtin_words(line)
        .into_iter()
        .find(|word| word.columns.contains(&column))
        .map(|word| word.word.doc().name)
}

#[test]
fn words_are_found_by_where_they_are_written() {
    let cases = [
        ("source image : Image [subject, visit]", "source", Some("source")),
        ("    stage clean:", "stage", Some("stage")),
        ("stage = merge(sorted @ vary(part))", "stage", None),
        ("path: results/{@product}/{@entities}.txt", "path", Some("path")),
        ("path image: input/{subject}.txt", "path", Some("path")),
        ("path image: input/{subject}.txt", "subject", None),
        ("path: {@stage}/{@product}/{@entities}.txt", "@stage", Some("@stage")),
        ("path: {@stage}/{@product}/{@entities}.txt", "@entities", Some("@entities")),
        ("path: sub-{sub}/{@labels}_{@product}", "@labels", Some("@labels")),
        ("path: out/{{@product}}", "@product", None),
        ("ext: .nii.gz", "ext", Some("ext")),
        ("ext: Image = convert(dicom)", "ext", None),
        ("operation mean(images: many Image @ min(2)) -> Image", "many", Some("many")),
        ("operation mean(images: many Image @ min(2)) -> Image", "min", Some("min")),
        (
            "operation strip(t1: Image) -> (brain: Image .nii.gz, mask: Image \"_mask.nii.gz\" beside brain)",
            "beside",
            Some("beside"),
        ),
        ("command process: tool --in {image} --out {@output}", "@output", Some("@output")),
        ("command process: tool --in {image} --out {@output}", "image", None),
        ("path: out/{output}", "output", None),
        ("path: out/{@output}", "@output", None),
        ("command process: tool {output} {@output}", "{output", None),
        ("command convert: dcm2niix -o {image.dir} -f {image.stem} {dicom}", ".dir", Some(".dir")),
        ("command convert: dcm2niix -o {image.dir} -f {image.stem} {dicom}", ".stem", Some(".stem")),
        ("command convert: dcm2niix -o {image.dir} -f {image.stem} {dicom}", "{ima", None),
        ("command copy: cp {input} {@output}  # {@output} again", "# {@output", None),
        ("verify register: check_same_grid {moving} {reference}", "verify", Some("verify")),
        ("calibrated = calibrate(reading, calibration @ where(revision=2))", "where", Some("where")),
        ("anomaly = compare(calibrated, reference @ same(station))", "same", Some("same")),
        ("forecast = predict(reading, model @ each(scenario))", "each", Some("each")),
        ("each = predict(reading)", "each", None),
        ("use shard, sort_lines from text.spit as text", "from", Some("use-from")),
        ("use shard, sort_lines from text.spit as text", " as", Some("use-as")),
        ("use as.spit", "as", None),
        ("dimensions [model, config, seed]", "dimensions", Some("dimensions")),
        ("sidecars photo [site]:", "sidecars", Some("sidecars")),
        ("    source photo_json .json", "source", Some("source")),
        ("pipeline analysis.spit", "pipeline", Some("pipeline")),
        ("root ../data", "root", Some("root")),
        ("discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}", "discover", Some("discover")),
        ("discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}", "dirs", Some("discover-from")),
        ("discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}", "sessions", None),
        ("require [subject, visit] where image count>=2", "count", Some("count")),
        ("require [subject, visit] where image count>=2", "where", Some("require-where")),
        ("require [subject, visit] where image has run=1,2", "has", Some("require-has")),
        ("require [subject, visit] where image count>=2", "image", None),
        ("drop [sub] where sessions count<2", "drop", Some("drop")),
        ("drop [sub] where sessions count<2", "where", Some("drop-where")),
        ("drop [sub, ses] where bold missing run=1,2", "missing", Some("missing")),
        ("drop [sub, ses] where bold has run=3", "has", Some("has")),
        ("exclude bold[sub=02,run=3]    # corrupted", "exclude", Some("exclude")),
        ("exclude bold[sub=02,run=3]    # corrupted", "corrupted", None),
        ("exclude from qc/excluded.csv", "from", Some("exclude-from")),
        ("contexts sessions:", "contexts", Some("contexts:")),
        ("contexts sessions:", "sessions", None),
        ("sources:", "sources", Some("sources:")),
        ("source_paths:", "source_paths", Some("source_paths:")),
        ("removed:", "removed", Some("removed:")),
    ];
    for (line, text, expected) in cases {
        assert_eq!(word_at(line, text), expected, "{line} at `{text}`");
    }
}

#[test]
fn columns_count_utf16_units_and_skip_a_byte_order_mark() {
    let words =
        builtin_words("\u{feff}source raw [id]\nout = copy(raw @ where(id=😀) @ vary(id))\n");
    assert_eq!((words[0].line, words[0].columns.clone()), (1, 2..8));
    let vary = words.iter().find(|word| word.word == Word::Vary).unwrap();
    // The emoji is two UTF-16 units.
    assert_eq!((vary.line, vary.columns.clone()), (2, 33..37));
}

#[test]
fn every_word_is_documented_once_and_links_to_a_section() {
    let names: BTreeSet<_> = DOCS.iter().map(|doc| doc.name).collect();
    assert_eq!(names.len(), DOCS.len(), "each word has its own name");
    assert_eq!(
        Word::Removed.doc().name,
        "removed:",
        "DOCS follows the order of Word"
    );
    let reference = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/language-reference.md"),
    )
    .unwrap();
    // GitHub's anchors: lowercase, punctuation but `-` and `_` dropped, and
    // spaces written as `-`.
    let anchors: BTreeSet<String> = reference
        .lines()
        .filter_map(|line| line.strip_prefix("##"))
        .map(|heading| {
            heading
                .trim_start_matches('#')
                .trim()
                .to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
                .map(|c| if c == ' ' { '-' } else { c })
                .collect()
        })
        .collect();
    for doc in &DOCS {
        assert!(
            anchors.contains(doc.anchor),
            "{}: no section #{}",
            doc.name,
            doc.anchor
        );
        assert!(
            !doc.summary.is_empty() && !doc.example.is_empty(),
            "{}",
            doc.name
        );
    }
}

fn check(file: &str, args: &[&str], text: &str) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", file])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn check_writes_each_words_documentation_once() {
    let text = "source raw [id]\nsource other [id]\noperation copy(input)\nout = copy(raw)\n";
    let (ok, json, _) = check("unsaved.spit", &["--json", "--stdin", "--hovers"], text);
    assert!(ok);
    assert!(json.contains(r#"{"line":1,"column":1,"end_column":7,"word":"source"}"#));
    assert!(json.contains(r#"{"line":2,"column":1,"end_column":7,"word":"source"}"#));
    assert_eq!(json.matches("Declares a product family").count(), 1);
    assert!(
        json.contains(r#""hovers":["#),
        "a pipeline keeps its names' hovers"
    );
    assert!(json.contains("language-reference.md#products-and-dimensions"));

    let (ok, json, _) = check("unsaved.spit", &["--json", "--stdin"], text);
    assert!(ok);
    assert!(
        !json.contains("\"words\""),
        "words are opt-in with --hovers"
    );
}

#[test]
fn a_recipe_and_a_spitout_have_words_too() {
    let (ok, json, _) = check(
        "unsaved.spitin",
        &["--json", "--stdin", "--hovers"],
        "require [id] where raw count>=1\n",
    );
    assert!(ok);
    assert!(json.contains("name the pipeline"), "{json}");
    assert!(json.contains(r#""word":"require""#));
    assert!(json.contains(r#""word":"require-where""#));
    assert!(
        !json.contains("\"hovers\""),
        "a recipe's names are its pipeline's"
    );

    let records = "sources:\n    raw[id=1]\n";
    let (ok, json, _) = check(
        "unsaved.spitout",
        &["--json", "--stdin", "--hovers"],
        records,
    );
    assert!(ok);
    assert!(
        json.starts_with(
            r#"{"diagnostics":[],"words":[{"line":1,"column":1,"end_column":8,"word":"sources:"}]"#
        ),
        "{json}"
    );

    // A `.spitout`'s syntax is checked on its own, with the bad text marked.
    let (ok, json, _) = check(
        "unsaved.spitout",
        &["--json", "--stdin"],
        "sources:\n    raw[id=1\n",
    );
    assert!(ok);
    assert!(
        json.contains(r#""source":"inventory","line":2,"column":9"#),
        "{json}"
    );
    let (ok, stdout, _) = check("unsaved.spitout", &["--stdin"], records);
    assert!(ok);
    assert_eq!(stdout, "Inputs valid.\n");
    let (ok, _, stderr) = check("unsaved.spitout", &["--stdin", "--path-rules"], records);
    assert!(!ok);
    assert!(stderr.contains("no path rules"), "{stderr}");
}
