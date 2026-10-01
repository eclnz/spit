//! SPIT's own path placeholders take `@`, as `{@product}`, so a reader can
//! tell them from the pipeline's dimensions, and no dimension name is
//! reserved.

mod support;

use spit::{diagnose, parse_pipeline, PathTemplate};
use support::{spit, Tree};

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.message)
        .collect()
}

#[test]
fn a_dimension_may_take_a_built_in_name() {
    let text = "path: out/{@stage}/{@product}/{stage}_{product}\nsource raw [stage, product]\npath raw: in/{stage}/{product}\noperation copy(a: A) -> A\nstage prep:\n    copied = copy(raw)\n";
    assert_eq!(errors(text), Vec::<String>::new());
    let pipeline = parse_pipeline(text).unwrap();
    assert_eq!(
        pipeline.path_template_for("copied").unwrap().to_string(),
        "out/{@stage}/{@product}/{stage}_{product}"
    );
}

#[test]
fn an_old_form_says_what_to_write() {
    for (name, meaning) in [
        ("product", "the product's name"),
        ("entities", "every dimension as `dimension=value`"),
        ("stage", "the stage that makes it"),
    ] {
        let text = format!(
            "path: out/{{{name}}}/{{@entities}}\nsource raw [id]\npath raw: in/{{id}}\noperation copy(a: A) -> A\ncopied = copy(raw)\n"
        );
        assert_eq!(
            errors(&text),
            [format!(
                "path template for `copied` uses absent dimension `{name}`; write `{{@{name}}}` for {meaning}"
            )]
        );
    }
}

#[test]
fn an_unknown_built_in_is_an_error() {
    let found = errors("path: out/{@name}/{@entities}\n");
    assert!(
        found.iter().any(|error| error.contains(
            "unknown built-in placeholder `{@name}`; path templates have `{@product}`, `{@entities}`, `{@labels}` and `{@stage}`"
        )),
        "{found:?}"
    );
}

#[test]
fn optional_groups_and_labels_resolve_for_each_cohort_product() {
    let pipeline = "examples/patterns/cohort/cohort.spit";
    let check = spit(&["check", pipeline, "--path-rules"]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let rules = String::from_utf8_lossy(&check.stdout);
    assert!(rules.contains("brain (output): default derivatives/sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_brain.nii.gz"), "{rules}");
    assert!(rules.contains("mc (output): default derivatives/sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_run-{run}_mc.nii.gz"), "{rules}");
    assert!(
        rules.contains("long (output): default derivatives/sub-{sub}/sub-{sub}_long.nii.gz"),
        "{rules}"
    );

    let hints = spit(&["check", pipeline, "--json"]);
    let json = String::from_utf8_lossy(&hints.stdout);
    assert!(json.contains("\"product\":\"long\",\"line\":"), "{json}");
    assert!(
        json.contains("\"path\":\"derivatives/sub-{sub}/sub-{sub}_long.nii.gz\""),
        "{json}"
    );

    let plan = spit(&["dag", "examples/patterns/cohort/cohort.spitin", "--paths"]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let paths = String::from_utf8_lossy(&plan.stdout);
    assert_eq!(paths.matches("Job ").count(), 24);
    for path in [
        "derivatives/sub-01/ses-01/anat/sub-01_ses-01_brain.nii.gz",
        "derivatives/sub-01/ses-01/func/sub-01_ses-01_run-1_mc.nii.gz",
        "derivatives/sub-01/ses-01/func/sub-01_ses-01_run-1_coreg.nii.gz",
        "derivatives/sub-01/ses-01/func/sub-01_ses-01_avg.nii.gz",
        "derivatives/sub-01/sub-01_long.nii.gz",
    ] {
        assert!(paths.contains(path), "missing {path}");
    }
}

#[test]
fn optional_groups_reject_unknown_dimensions_and_invalid_nesting() {
    let template = PathTemplate::parse("out/[[literal]]_[{ses}_]{@labels}").unwrap();
    assert_eq!(template.as_str(), "out/[[literal]]_[{ses}_]{@labels}");
    for text in ["out/[x]/{@product}", "out/[{ses}[{run}]]", "out/[{ses}"] {
        assert!(PathTemplate::parse(text).is_err(), "{text}");
    }
    let found = errors("path: out/[ses-{sess}]{@product}.txt\nsource raw [ses]\n");
    assert!(
        found
            .iter()
            .any(|error| error.contains("path rule names `{sess}`, which no product has")),
        "{found:?}"
    );
}

#[test]
fn a_dimensionless_product_omits_an_optional_label_group() {
    let text = "path: out/[{@labels}_]{@product}.txt\nsource singleton\n";
    assert!(errors(text).is_empty());
    let pipeline = parse_pipeline(text).unwrap();
    assert_eq!(
        pipeline.path_template_for("singleton").unwrap().as_str(),
        "out/{@product}.txt"
    );

    let missing = errors("path: out/{@labels}.txt\nsource singleton\n");
    assert!(
        missing
            .iter()
            .any(|error| error.contains("has no dimensions; put it in `[...]`")),
        "{missing:?}"
    );
}

#[test]
fn labels_warn_when_a_value_contains_a_dash() {
    let tree = Tree::new("dashed-label", &["data/in/sub-01-a.txt"]);
    tree.write(
        "pipeline.spit",
        "path: out/{@product}/{@entities}.txt\nsource raw [sub]\npath raw: in/{@labels}.txt\noperation copy(input)\ncommand copy: cp {input} {@output}\nresult = copy(raw)\n",
    );
    let recipe = tree.write("recipe.spitin", "pipeline pipeline.spit\nroot data\n");
    let result = spit(&["dag", recipe.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let warnings = String::from_utf8_lossy(&result.stderr);
    assert!(
        warnings.contains("`{@labels}` writes `sub-01-a`"),
        "{warnings}"
    );
}

#[test]
fn an_explicit_optional_path_has_an_editor_hint() {
    let tree = Tree::new("explicit-optional-path", &[]);
    let pipeline = tree.write(
        "pipeline.spit",
        "source raw [sub]\npath raw: in/{sub}.txt\noperation copy(input)\ncommand copy: cp {input} {@output}\npath result: out/{sub}[/{@stage}]/result.txt\nresult = copy(raw)\n",
    );
    let output = spit(&["check", pipeline.to_str().unwrap(), "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8_lossy(&output.stdout);
    assert!(json.contains("\"path\":\"out/{sub}/result.txt\""), "{json}");
}

#[test]
fn discovery_uses_each_sources_resolved_groups_and_labels() {
    let tree = Tree::new(
        "optional-source-paths",
        &[
            "data/in/sub-01/ses-02/sub-01_ses-02_image.txt",
            "data/in/sub-01/sub-01_metadata.txt",
        ],
    );
    tree.write(
        "pipeline.spit",
        "path: in/sub-{sub}[/ses-{ses}]/{@labels}_{@product}.txt\nsource image [sub, ses]\nsource metadata [sub]\n",
    );
    let recipe = tree.write("recipe.spitin", "pipeline pipeline.spit\nroot data\n");
    let result = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let inventory = String::from_utf8_lossy(&result.stdout);
    assert!(inventory.contains("image[sub=01,ses=02]"), "{inventory}");
    assert!(inventory.contains("metadata[sub=01]"), "{inventory}");
}
