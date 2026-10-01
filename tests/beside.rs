//! An output a tool writes beside another without being told where, such
//! as the `.json` dcm2niix writes next to its image, follows that file's
//! path, and its command need not mention it.

mod support;

use spit::{diagnose, parse_pipeline, Pipeline};
use support::{spit, text, Tree};

/// The path `product` is written to, as its rules and extensions give it.
fn path(pipeline: &Pipeline, product: &str) -> String {
    pipeline.path_template_for(product).unwrap().to_string()
}

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.message)
        .collect()
}

const CONVERT: &str = "\
path: out/{@product}/{@entities}
source dicom [sub]
path dicom: in/{sub}
operation convert(dicom) -> (image: Image .nii.gz, meta: Json .json beside image)
command convert: convert {dicom} {image}
operation strip(t1: Image) -> (brain: Image .nii.gz, mask: Image \"_mask.nii.gz\" beside brain)
command strip: bet {t1} {brain} -m
image, meta = convert(dicom)
brain, mask = strip(image)
";

#[test]
fn a_beside_output_takes_its_siblings_path_and_name() {
    let pipeline = parse_pipeline(CONVERT).unwrap();
    // `{@product}` is the sibling's name: the tool writes beside its file.
    assert_eq!(
        path(&pipeline, "image"),
        "out/{@product}/{@entities}.nii.gz"
    );
    assert_eq!(path(&pipeline, "meta"), "out/image/{@entities}.json");
    assert_eq!(path(&pipeline, "mask"), "out/brain/{@entities}_mask.nii.gz");
    // Its extension is its suffix's, from the first `.`.
    let strip = pipeline
        .operations
        .iter()
        .find(|operation| operation.name == "strip")
        .unwrap();
    assert_eq!(strip.outputs[1].extension.as_deref(), Some(".nii.gz"));
}

#[test]
fn a_beside_output_follows_its_siblings_own_rule() {
    let pipeline = parse_pipeline(&format!(
        "{CONVERT}path brain: derivatives/sub-{{sub}}/anat/sub-{{sub}}_brain\n"
    ))
    .unwrap();
    assert_eq!(
        path(&pipeline, "mask"),
        "derivatives/sub-{sub}/anat/sub-{sub}_brain_mask.nii.gz"
    );
    let staged = parse_pipeline(
        "path: {@stage}/{@product}/{@entities}\nsource dicom [sub]\npath dicom: in/{sub}\n\
         operation convert(dicom) -> (image: Image .nii.gz, meta .json beside image)\n\
         stage import:\n    image, meta = convert(dicom)\n",
    )
    .unwrap();
    assert_eq!(path(&staged, "meta"), "{@stage}/image/{@entities}.json");
}

#[test]
fn the_command_may_leave_a_beside_output_out_and_the_plan_keeps_it() {
    let tree = Tree::new("beside-plan", &[]);
    let pipeline = tree.write("pipeline.spit", CONVERT);
    let inputs = tree.write("inputs.spitout", "sources:\n    dicom[sub=01]\n");
    let output = spit(&[
        "dag",
        pipeline.to_str().unwrap(),
        inputs.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let plan = text(&output.stdout);
    assert!(
        plan.contains("\"path\":\"out/image/sub=01.json\""),
        "{plan}"
    );
    assert!(
        plan.contains("\"path\":\"out/brain/sub=01_mask.nii.gz\""),
        "{plan}"
    );
    let output = spit(&["check", pipeline.to_str().unwrap(), "--path-rules"]);
    let rules = text(&output.stdout);
    assert!(
        rules.contains("  meta (output): beside image out/image/{@entities}.json\n"),
        "{rules}"
    );
    // A command that does name it is fine too.
    assert_eq!(
        errors(&CONVERT.replace("convert {dicom} {image}", "convert {dicom} {image} {meta}")),
        Vec::<String>::new()
    );
    // Every other output must still be written.
    assert_eq!(
        errors(&CONVERT.replace("convert {dicom} {image}", "convert {dicom}")),
        ["command for `convert` must use `{image}`"]
    );
}

#[test]
fn a_beside_output_has_no_path_rule_of_its_own() {
    assert_eq!(
        errors(&format!("{CONVERT}path meta: elsewhere/{{sub}}.json\n")),
        ["`meta` is written beside `image`, so its path follows `image`'s; remove its path rule"]
    );
}

#[test]
fn a_beside_output_names_a_sibling_with_an_extension() {
    for (outputs, message) in [
        (
            "(image: Image .nii.gz, meta .json beside picture)",
            "`meta` is written beside `picture`, which is not an output of this operation",
        ),
        (
            "(image: Image .nii.gz, meta .json beside image, log .txt beside meta)",
            "`log` is written beside `meta`, which is itself written beside another",
        ),
        (
            "(image: Image, meta .json beside image)",
            "`meta` is written beside `image`, which declares no extension for `meta` to replace",
        ),
        (
            "Json .json beside image",
            "`beside` names another output of the same operation",
        ),
        (
            "(image: Image .nii.gz, meta: Json beside image)",
            "names what its file name ends with",
        ),
        (
            "(image: Image .nii.gz, mask \"/mask\" beside image)",
            "cannot end a file name",
        ),
    ] {
        let error =
            parse_pipeline(&format!("operation convert(dicom) -> {outputs}\n")).unwrap_err();
        assert!(error.to_string().contains(message), "{outputs}: {error}");
    }
}
