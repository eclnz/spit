//! A `sidecars` block declares sources that share dimensions and a path
//! stem and differ only by extension, as a photo and its GPS track. The
//! stem is the block's `path:` line, or else the recipe's `path name:`.

mod support;

use std::process::Command;

use spit::{parse_pipeline, PathTemplate, SidecarGroup};
use support::{text, Tree};

const PHOTOS: &str = "\
sidecars photo [site, shot]:
    path: site-{site}/shot-{shot}
    source raw : Image .raw
    source gps : Track .gpx  # the pose
    source meta .json
operation load(image: Image, gps: Track, meta) -> Image
command load: load {image} {gps} {meta} {@output}
loaded = load(raw, gps, meta)
";

/// `PHOTOS` with no stem, for a recipe to give one.
const UNPLACED: &str = "\
sidecars photo [site, shot]:
    source raw : Image .raw
    source gps : Track .gpx
    source meta .json
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
fn members_are_sources_with_the_groups_dimensions_and_stem() {
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
            pipeline.product_paths[member],
            format!("site-{{site}}/shot-{{shot}}{extension}").as_str()
        );
    }
    assert_eq!(
        pipeline.sidecar_groups,
        [SidecarGroup {
            name: "photo".to_owned(),
            dimensions: vec!["site".to_owned(), "shot".to_owned()],
            members: vec![
                ("raw".to_owned(), ".raw".to_owned()),
                ("gps".to_owned(), ".gpx".to_owned()),
                ("meta".to_owned(), ".json".to_owned()),
            ],
            stem: Some(PathTemplate::parse("site-{site}/shot-{shot}").unwrap()),
        }]
    );
}

#[test]
fn a_group_without_a_stem_leaves_its_members_to_the_recipe() {
    let pipeline = parse_pipeline(UNPLACED).unwrap();
    assert_eq!(pipeline.sidecar_groups[0].stem, None);
    for member in ["raw", "gps", "meta"] {
        assert!(!pipeline.product_paths.contains_key(member));
    }
}

#[test]
fn a_group_ends_at_the_first_line_not_indented_beneath_it() {
    let pipeline = parse_pipeline(
        "sidecars config:\n    path: config/settings\n    source settings .toml\n    source schema .json\n\nsource other [id]\n",
    )
    .unwrap();
    // A group may have no dimensions, for one set of files.
    assert_eq!(pipeline.product_paths["schema"], "config/settings.json");
    assert!(!pipeline.product_paths.contains_key("other"));
}

#[test]
fn a_groups_lines_are_checked() {
    for (text, message) in [
        (
            "sidecars photo [id]:\n    source raw : Image [id] .raw\n",
            "takes the group's dimensions; remove its own",
        ),
        (
            "sidecars photo [id]:\n    source raw : Image\n",
            "names the extension its file adds to the stem",
        ),
        (
            "sidecars photo [id]:\n    operation f(x)\n",
            "holds only its `path:` stem and its sources",
        ),
        ("sidecars photo [id]:\nsource raw [id]\n", "has no sources"),
        ("sidecars photo [id]:\n    path: p/{id}\n", "has no sources"),
        ("sidecars photo [id]\n", "expected `sidecars name [dimensions]:`"),
        (
            "sidecars photo [id]: p/{id}\n    source raw .raw\n",
            "a `sidecars` header ends at its `:`",
        ),
        ("sidecars photo [id]:\n    path: p/{id\n    source raw .raw\n", "unclosed `{`"),
        (
            "sidecars photo [id]:\n    source raw .raw\n    path: p/{id}\n",
            "comes before its sources",
        ),
        (
            "sidecars photo [id]:\n    path: p/{id}\n    path: q/{id}\n    source raw .raw\n",
            "has one `path:` line",
        ),
        (
            "sidecars photo [id]:\n    path raw: p/{id}\n    source raw .raw\n",
            "gives its stem as `path: stem`",
        ),
        (
            "sidecars photo [id]:\n    path p/{id}\n    source raw .raw\n",
            "gives its stem as `path: stem`",
        ),
        (
            "stage s:\n    sidecars photo [id]:\n",
            "belongs at the top level",
        ),
        (
            "sidecars photo [id]:\n    source raw .raw\nsidecars photo [id]:\n    source gps .gpx\n",
            "duplicate sidecars group `photo`",
        ),
        (
            "sidecars photo [id]:\n    path: p/{id}\n    source raw .raw\npath raw: elsewhere/{id}.raw\n",
            "duplicate path template for product `raw`",
        ),
        (
            "sidecars photo [id]:\n    source raw .raw\npath raw: elsewhere/{id}.raw\n",
            "source `raw` takes its path from sidecars group `photo`",
        ),
        (
            "sidecars photo [id]:\n    source raw .raw\npath photo: p/{id}\n",
            "gives its stem on an indented `path:` line in its block",
        ),
        (
            "sidecars photo [id]:\n    source raw .raw\nsource photo [id]\n",
            "shares its name with a product",
        ),
        (
            "sidecars photo [id]:\n    source raw .raw\noperation f(x) -> Y\nphoto = f(raw)\n",
            "shares its name with a product",
        ),
        (
            "sidecars photo [id]:\n    path: p/{id}\n    source raw .raw\n    source copy .raw\n",
            "bind to the same path",
        ),
    ] {
        let found = spit::diagnose(text, None);
        assert!(
            found.iter().any(|diagnostic| diagnostic.is_error()
                && diagnostic.message.to_lowercase().contains(message)),
            "{text}: {found:?}"
        );
    }
    // A product may still be called `sidecars`.
    let step = parse_pipeline("source raw [id]\noperation f(x)\nsidecars = f(raw)\n").unwrap();
    assert!(step
        .products
        .iter()
        .any(|product| product.name == "sidecars"));
}

#[test]
fn a_recipe_cannot_declare_a_group() {
    let error = spit::parse_input_spec("pipeline a.spit\nsidecars photo [id]:\n").unwrap_err();
    assert!(
        error.to_string().contains("belong in the .spit pipeline"),
        "{error}"
    );
}

#[test]
fn a_recipe_gives_a_groups_stem() {
    let tree = Tree::new("sidecars-recipe-stem", &FILES);
    tree.write("pipeline.spit", UNPLACED);
    let recipe = tree.write(
        "dataset.spitin",
        "pipeline pipeline.spit\npath photo: site-{site}/shot-{shot}\n",
    );
    let recipe = recipe.to_str().unwrap();
    let (ok, out, err) = spit(&["inputs", recipe]);
    assert!(ok, "{err}");
    // The `.spitout` writes each member's rule, for a DAG read without the
    // recipe.
    assert!(
        out.contains(
            "source_paths:\n    gps: site-{site}/shot-{shot}.gpx\n    meta: site-{site}/shot-{shot}.json\n    raw: site-{site}/shot-{shot}.raw\n"
        ),
        "{out}"
    );
    assert!(out.contains("    raw[site=a,shot=2]\n"), "{out}");
    let (ok, out, err) = spit(&["check", recipe, "--path-rules"]);
    assert!(ok, "{err}");
    assert!(out.contains("site-{site}/shot-{shot}.gpx"), "{out}");
}

#[test]
fn a_recipes_default_gives_a_group_one_stem_named_for_it() {
    let tree = Tree::new(
        "sidecars-recipe-default",
        &[
            "site-a/shot-1/photo.raw",
            "site-a/shot-1/photo.gpx",
            "site-a/shot-1/photo.json",
        ],
    );
    tree.write("pipeline.spit", UNPLACED);
    let recipe = tree.write(
        "dataset.spitin",
        "pipeline pipeline.spit\npath: site-{site}/shot-{shot}/{@product}\n",
    );
    let (ok, out, err) = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(
        out.contains("    raw: site-{site}/shot-{shot}/photo.raw\n"),
        "{out}"
    );
    assert!(
        out.contains("    gps: site-{site}/shot-{shot}/photo.gpx\n"),
        "{out}"
    );
}

#[test]
fn a_recipes_group_path_is_checked() {
    for (pipeline, recipe, message) in [
        (
            PHOTOS,
            "path photo: elsewhere/{site}/{shot}\n",
            "`photo` has path rules in both .spit and .spitin",
        ),
        (
            UNPLACED,
            "path raw: site-{site}/shot-{shot}.raw\n",
            "source `raw` takes its path from sidecars group `photo`; write `path photo:`",
        ),
        (
            UNPLACED,
            "path loaded: x/{site}/{shot}\n",
            "must name a source product or sidecars group",
        ),
    ] {
        let tree = Tree::new("sidecars-recipe-errors", &[]);
        tree.write("pipeline.spit", pipeline);
        let recipe = tree.write(
            "dataset.spitin",
            &format!("pipeline pipeline.spit\n{recipe}"),
        );
        let (ok, out, err) = spit(&["check", recipe.to_str().unwrap()]);
        assert!(!ok, "{recipe:?}: {out}");
        assert!(err.contains(message), "{message}: {err}");
    }
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
        "pipeline pipeline.spit\nexclude meta[site=a,shot=3]\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["inputs", recipe.to_str().unwrap()])
        .output()
        .unwrap();
    let notes = text(&output.stderr);
    assert!(output.status.success(), "{notes}");
    assert!(
        notes.contains("warning: photo[site=a,shot=2] has .raw and .gpx but no .json\n"),
        "{notes}"
    );
    assert_eq!(notes.matches("warning:").count(), 1, "{notes}");
}
