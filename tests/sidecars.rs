//! A source declared beside a file source follows its path and dimensions.
//! Companions remain ordinary sources; discovery reports incomplete sets.

mod support;

use std::process::Command;

use spit::parse_pipeline;
use support::{text, Tree};

const PHOTOS: &str = "\
source raw : Image .raw [site, shot]
path raw: site-{site}/shot-{shot}.raw
source gps : Track .gpx beside raw  # the pose
source meta .json beside raw
operation load(image: Image, gps: Track, meta) -> Image
command load: load {image} {gps} {meta} {@output}
loaded = load(raw, gps, meta)
";

const UNPLACED: &str = "\
source raw : Image .raw [site, shot]
source gps : Track .gpx beside raw
source meta .json beside raw
operation load(image: Image, gps: Track, meta) -> Image
command load: load {image} {gps} {meta} {@output}
loaded = load(raw, gps, meta)
";

const FILES: [&str; 6] = [
    "site-a/shot-1.raw",
    "site-a/shot-1.gpx",
    "site-a/shot-1.json",
    "site-a/shot-2.raw",
    "site-a/shot-2.gpx",
    "site-a/shot-2.json",
];

fn spit(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .output()
        .unwrap();
    (
        output.status.success(),
        text(&output.stdout),
        text(&output.stderr),
    )
}

#[test]
fn companions_inherit_dimensions_and_follow_the_anchor_path() {
    let pipeline = parse_pipeline(PHOTOS).unwrap();
    for (member, extension) in [("raw", ".raw"), ("gps", ".gpx"), ("meta", ".json")] {
        let product = pipeline
            .products
            .iter()
            .find(|product| product.name == member)
            .unwrap();
        assert_eq!(product.dimensions, ["site", "shot"]);
        assert!(pipeline.is_source(member));
        assert_eq!(
            pipeline.path_template_for(member).unwrap().to_string(),
            format!("site-{{site}}/shot-{{shot}}{extension}")
        );
    }
    assert_eq!(pipeline.sidecar_groups[0].name, "raw");
    assert_eq!(pipeline.sidecar_groups[0].members.len(), 3);
}

#[test]
fn a_quoted_companion_suffix_is_appended_to_the_main_stem() {
    let pipeline = parse_pipeline(
        "source image .nii.gz [sub]\npath image: sub-{sub}_T1w.nii.gz\nsource mask \"_mask.nii.gz\" beside image\n",
    )
    .unwrap();
    assert_eq!(
        pipeline.path_template_for("mask").unwrap().to_string(),
        "sub-{sub}_T1w_mask.nii.gz"
    );
}

#[test]
fn source_beside_requires_a_prior_file_anchor_and_its_own_suffix() {
    for (source, message) in [
        ("source meta .json beside raw", "unknown source `raw`"),
        (
            "source raw .raw
source meta beside raw",
            "names what its file name ends with",
        ),
        (
            "source raw /
source meta .json beside raw",
            "file source with an extension",
        ),
        (
            "source raw .raw
source meta .json [id] beside raw",
            "inherits its dimensions",
        ),
        (
            "source raw .raw
source meta .json beside raw
path meta: m.json",
            "path follows that source",
        ),
    ] {
        let error = parse_pipeline(&format!("{source}\n")).unwrap_err();
        assert!(error.to_string().contains(message), "{source}: {error}");
    }
}

#[test]
fn a_recipe_gives_the_anchor_path() {
    let tree = Tree::new("sidecars-recipe-path", &FILES);
    tree.write("pipeline.spit", UNPLACED);
    let recipe = tree.write(
        "dataset.spitin",
        "pipeline pipeline.spit\nroot .\npath raw: site-{site}/shot-{shot}.raw\n",
    );
    let recipe = recipe.to_str().unwrap();
    let (ok, out, err) = spit(&["inputs", recipe]);
    assert!(ok, "{err}");
    assert!(
        out.contains("source_paths:\n    raw: site-{site}/shot-{shot}.raw\n"),
        "{out}"
    );
    assert!(out.contains("    raw[site=a,shot=2]\n"), "{out}");
    let (ok, out, err) = spit(&["check", recipe, "--path-rules"]);
    assert!(ok, "{err}");
    assert!(out.contains("site-{site}/shot-{shot}.gpx"), "{out}");
}

#[test]
fn a_recipe_default_places_each_companion_at_the_anchor_name() {
    let tree = Tree::new(
        "sidecars-recipe-default",
        &[
            "site-a/shot-1/raw.raw",
            "site-a/shot-1/raw.gpx",
            "site-a/shot-1/raw.json",
        ],
    );
    tree.write("pipeline.spit", UNPLACED);
    let recipe = tree.write(
        "dataset.spitin",
        "pipeline pipeline.spit\nroot .\npath: site-{site}/shot-{shot}/{@product}\n",
    );
    let (ok, out, err) = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(
        out.contains("    raw: site-{site}/shot-{shot}/{@product}\n"),
        "{out}"
    );
    // Companions derive from that source path and need no independent rule.
    assert!(!out.contains("    gps: site-"), "{out}");
    assert!(out.contains("    gps[site=a,shot=1]\n"), "{out}");
}

#[test]
fn a_recipe_cannot_override_a_companion_path() {
    for (pipeline, recipe, message) in [
        (
            PHOTOS,
            "path raw: elsewhere/{site}/{shot}.raw\n",
            "path rules in both",
        ),
        (UNPLACED, "path gps: x.gpx\n", "beside `raw`"),
        (UNPLACED, "path loaded: x\n", "made by a step"),
        (UNPLACED, "path missing: x\n", "must name a source product"),
    ] {
        let tree = Tree::new("sidecars-recipe-errors", &[]);
        tree.write("pipeline.spit", pipeline);
        let recipe = tree.write(
            "dataset.spitin",
            &format!("pipeline pipeline.spit\nroot .\n{recipe}"),
        );
        let (ok, out, err) = spit(&["check", recipe.to_str().unwrap()]);
        assert!(!ok, "{recipe:?}: {out}");
        assert!(err.contains(message), "{message}: {err}");
    }
}

#[test]
fn a_spitout_cannot_override_a_companion_path() {
    let tree = Tree::new("beside-inventory-path", &[]);
    let pipeline = tree.write("pipeline.spit", UNPLACED);
    let inputs = tree.write(
        "inputs.spitout",
        "source_paths:\n    raw: site-{site}/shot-{shot}.raw\n    gps: elsewhere/{site}/{shot}.gpx\n",
    );
    let (ok, _, err) = spit(&[
        "artifacts",
        pipeline.to_str().unwrap(),
        inputs.to_str().unwrap(),
    ]);
    assert!(!ok);
    assert!(err.contains("source `gps` is beside `raw`"), "{err}");
}

#[test]
fn discovery_names_a_group_missing_a_file() {
    let tree = Tree::new(
        "sidecars-incomplete",
        &[
            "site-a/shot-1.raw",
            "site-a/shot-1.gpx",
            "site-a/shot-1.json",
            // No `.json`: the group is incomplete.
            "site-a/shot-2.raw",
            "site-a/shot-2.gpx",
            // Its `.json` is excluded on purpose, so no warning.
            "site-a/shot-3.raw",
            "site-a/shot-3.gpx",
            "site-a/shot-3.json",
        ],
    );
    tree.write("pipeline.spit", PHOTOS);
    let recipe = tree.write(
        "dataset.spitin",
        "pipeline pipeline.spit\nroot .\nexclude meta[site=a,shot=3]\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["inputs", recipe.to_str().unwrap()])
        .output()
        .unwrap();
    let notes = text(&output.stderr);
    assert!(output.status.success(), "{notes}");
    assert!(
        notes.contains("warning: raw[site=a,shot=2] has .raw and .gpx but no .json\n"),
        "{notes}"
    );
    assert_eq!(notes.matches("warning:").count(), 1, "{notes}");
}

#[test]
fn discovery_names_incomplete_groups_in_value_order() {
    let tree = Tree::new(
        "sidecars-incomplete-order",
        &[
            "site-b/shot-10.raw",
            "site-b/shot-10.gpx",
            "site-b/shot-2.raw",
            "site-b/shot-2.json",
            "site-a/shot-10.raw",
            "site-a/shot-10.gpx",
            "site-a/shot-10.json",
            // A rule removes only part of a group: the `.json` it removes
            // is not missing, the `.gpx` still is.
            "site-a/shot-3.raw",
            "site-a/shot-3.json",
        ],
    );
    tree.write("pipeline.spit", PHOTOS);
    let recipe = tree.write(
        "dataset.spitin",
        "pipeline pipeline.spit\nroot .\nexclude meta[site=a,shot=3]\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["inputs", recipe.to_str().unwrap()])
        .output()
        .unwrap();
    let notes = text(&output.stderr);
    assert!(output.status.success(), "{notes}");
    let warnings: Vec<_> = notes
        .lines()
        .filter(|line| line.starts_with("warning:"))
        .collect();
    assert_eq!(
        warnings,
        [
            "warning: raw[site=a,shot=3] has .raw but no .gpx",
            "warning: raw[site=b,shot=2] has .raw and .json but no .gpx",
            "warning: raw[site=b,shot=10] has .raw and .gpx but no .json",
        ],
        "{notes}"
    );
}

/// Records of two shots: the first complete, the second without its `.json`.
const SHOT_RECORDS: &str = "\
sources:
    raw[site=a,shot=1]
    gps[site=a,shot=1]
    meta[site=a,shot=1]
    raw[site=a,shot=2]
    gps[site=a,shot=2]
";

#[test]
fn records_name_a_group_missing_a_file() {
    let tree = Tree::new("sidecars-incomplete-records", &FILES);
    tree.write("pipeline.spit", PHOTOS);
    let recipe = tree.write(
        "dataset.spitin",
        &format!("pipeline pipeline.spit\nroot .\n{SHOT_RECORDS}"),
    );
    let (ok, _, err) = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(
        err.contains("warning: raw[site=a,shot=2] has .raw and .gpx but no .json\n"),
        "{err}"
    );

    // A `.spitout` with the same records warns when a command reads it.
    let pipeline = tree.path().join("pipeline.spit");
    let inputs = tree.write("inputs.spitout", SHOT_RECORDS);
    let (ok, _, err) = spit(&[
        "artifacts",
        pipeline.to_str().unwrap(),
        inputs.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    assert_eq!(
        err.matches("warning: raw[site=a,shot=2] has .raw and .gpx but no .json\n")
            .count(),
        1,
        "{err}"
    );
}

#[test]
fn records_count_what_an_earlier_run_removed() {
    let tree = Tree::new("sidecars-removed-records", &[]);
    let pipeline = tree.write("pipeline.spit", PHOTOS);
    let inputs = tree.write(
        "inputs.spitout",
        &format!(
            "{SHOT_RECORDS}removed:\n    meta[site=a,shot=2]\n        rule: exclude meta[site=a,shot=2]\n        at: line 3\n"
        ),
    );
    let (ok, _, err) = spit(&[
        "artifacts",
        pipeline.to_str().unwrap(),
        inputs.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    assert!(!err.contains("but no"), "{err}");
}

/// `PHOTOS` with a step that reads the image alone, beside the one that
/// reads every member.
const TWO_STEPS: &str = "\
path: out/{@product}/site-{site}_shot-{shot}
source raw : Image .raw [site, shot]
path raw: site-{site}/shot-{shot}.raw
source gps : Track .gpx beside raw
source meta .json beside raw
operation load(image: Image, gps: Track, meta) -> Image
command load: load {image} {gps} {meta} {@output}
operation thumb(image: Image) -> Image
command thumb: thumb {image} {@output}
loaded = load(raw, gps, meta)
small = thumb(raw)
";

#[test]
fn a_missing_member_fails_only_the_steps_that_read_it() {
    // Shot 2 has no `.json`. No job runs with an input left out: `dag`
    // stops at `load`, `--partial` plans the rest, and `drop` removes the
    // group on the record.
    let tree = Tree::new("sidecars-missing-member", &FILES[..5]);
    tree.write("pipeline.spit", TWO_STEPS);
    let recipe = tree.write("dataset.spitin", "pipeline pipeline.spit\nroot .\n");
    let recipe = recipe.to_str().unwrap();

    let (ok, _, err) = spit(&["dag", recipe]);
    assert!(!ok);
    assert!(
        err.contains("warning: raw[site=a,shot=2] has .raw and .gpx but no .json\n"),
        "{err}"
    );
    assert!(
        err.contains("no `meta` artifact for input `meta` of `load` at [shot=2,site=a]"),
        "{err}"
    );

    let (ok, out, err) = spit(&["dag", recipe, "--partial", "--json"]);
    assert!(ok, "{err}");
    for path in [
        "out/small/site-a_shot-1",
        "out/small/site-a_shot-2",
        "out/loaded/site-a_shot-1",
    ] {
        assert!(
            out.contains(&format!("\"path\":\"{path}\"")),
            "{path}: {out}"
        );
    }
    assert!(!out.contains("out/loaded/site-a_shot-2"), "{out}");
    assert!(
        out.contains("\"left_out\":[{\"identity\":\"loaded[shot=2,site=a]\",\"reasons\":[\"no `meta` artifact for input `meta` of `load` at [shot=2,site=a]\"]}]"),
        "{out}"
    );

    let dropped = tree.write(
        "dropped.spitin",
        "pipeline pipeline.spit\nroot .\ndrop [site, shot] where meta count=0\n",
    );
    let (ok, out, err) = spit(&["dag", dropped.to_str().unwrap(), "--json"]);
    assert!(ok, "{err}");
    assert!(
        err.contains("dropped [site=a,shot=2] by `drop [site, shot] where meta count=0`"),
        "{err}"
    );
    assert!(!err.contains("but no"), "{err}");
    assert!(!out.contains("shot-2"), "{out}");
    assert!(out.contains("\"left_out\":[]"), "{out}");
}
