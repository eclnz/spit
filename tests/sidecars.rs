//! A `sidecars` block declares sources that share dimensions and a path
//! stem and differ only by extension, as a photo and its GPS track.

mod support;

use std::process::Command;

use spit::{parse_pipeline, SidecarGroup};
use support::{text, Tree};

const PHOTOS: &str = "\
sidecars photo [site, shot]: site-{site}/shot-{shot}
    source raw : Image .raw
    source gps : Track .gpx  # the pose
    source meta .json
operation load(image: Image, gps: Track, meta) -> Image
command load: load {image} {gps} {meta} {output}
loaded = load(raw, gps, meta)
";

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
        }]
    );
}

#[test]
fn a_group_ends_at_the_first_line_not_indented_beneath_it() {
    let pipeline = parse_pipeline(
        "sidecars config: config/settings\n    source settings .toml\n    source schema .json\n\nsource other [id]\n",
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
            "sidecars photo [id]: p/{id}\n    source raw : Image [id] .raw\n",
            "takes the group's dimensions; remove its own",
        ),
        (
            "sidecars photo [id]: p/{id}\n    source raw : Image\n",
            "names the extension its file adds to the stem",
        ),
        (
            "sidecars photo [id]: p/{id}\n    operation f(x)\n",
            "holds only its sources",
        ),
        ("sidecars photo [id]: p/{id}\nsource raw [id]\n", "has no sources"),
        ("sidecars photo [id]: p/{id}\n", "has no sources"),
        ("sidecars photo [id]\n", "expected `sidecars name [dimensions]: path stem`"),
        ("sidecars photo [id]: p/{id\n", "unclosed `{`"),
        (
            "stage s:\n    sidecars photo [id]: p/{id}\n",
            "belongs at the top level",
        ),
        (
            "sidecars photo [id]: p/{id}\n    source raw .raw\nsidecars photo [id]: q/{id}\n    source gps .gpx\n",
            "duplicate sidecars group `photo`",
        ),
        (
            "sidecars photo [id]: p/{id}\n    source raw .raw\npath raw: elsewhere/{id}.raw\n",
            "duplicate path template for product `raw`",
        ),
        (
            "sidecars photo [id]: p/{id}\n    source raw .raw\n    source copy .raw\n",
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
    let error =
        spit::parse_input_spec("pipeline a.spit\nsidecars photo [id]: p/{id}\n").unwrap_err();
    assert!(
        error.to_string().contains("belong in the .spit pipeline"),
        "{error}"
    );
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
