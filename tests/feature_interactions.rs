//! Features that each work alone, used together: a folder source, a source
//! `check`, a `verify`, a `beside` output, stages and a `many` input
//! with `@ min`, planned with and without `--partial`.

mod support;

use support::{spit, text, Tree};

const PIPELINE: &str = "\
source dicom : Dicom / [sub]
path dicom: dicom/sub={sub}
source mask .nii [sub]
path mask: mask/{sub}.nii
check nonempty: test -s {@path}
operation convert(dicom: Dicom, mask: Mask @ check(nonempty)) -> (image: Image .nii.gz, meta: Json .json beside image)
command convert: dcm2niix -o {image.dir} -f {image.stem} {dicom} {mask}
verify convert: test -d {dicom}
operation avg(images: many Image @ min(2)) -> Image .nii.gz
command avg: avg {images} {@output}
path: out/{@product}/{@entities}
stage pre:
    image, meta = convert(dicom, mask)
stage post:
    mean = avg(image @ vary(sub))
";

/// Subject 03 has a folder but no mask.
fn dataset() -> (Tree, String) {
    let tree = Tree::new(
        "interactions",
        &[
            "d/dicom/sub=01/x.dcm",
            "d/dicom/sub=02/x.dcm",
            "d/dicom/sub=03/x.dcm",
            "d/mask/01.nii",
            "d/mask/02.nii",
        ],
    );
    tree.write("p.spit", PIPELINE);
    let recipe = tree.write("p.spitin", "pipeline p.spit\nroot d\n");
    let recipe = recipe.to_str().unwrap().to_owned();
    (tree, recipe)
}

#[test]
fn a_missing_source_stops_the_plan_and_names_what_it_holds_back() {
    let (_tree, recipe) = dataset();
    let output = spit(&["dag", &recipe]);
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("no `mask` artifact for input `mask` of `convert` at [sub=03]"),
        "{stderr}"
    );
}

#[test]
fn a_partial_plan_leaves_out_a_job_with_its_beside_output_and_aggregates_the_rest() {
    let (_tree, recipe) = dataset();
    let output = spit(&["dag", &recipe, "--partial", "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let plan = text(&output.stdout);
    // The folder source is an external input, with its kind.
    assert!(
        plan.contains("\"path\":\"dicom/sub=01\",\"kind\":\"folder\""),
        "{plan}"
    );
    // Both outputs of the job that cannot run are left out together.
    let left_out = &plan[plan.find("\"left_out\"").expect("a left_out array")..];
    for identity in ["image[sub=03]", "meta[sub=03]"] {
        assert!(
            left_out.contains(&format!("\"identity\":\"{identity}\"")),
            "{left_out}"
        );
    }
    // The aggregate runs over the two complete members, `meta` is written
    // beside its image, and both checks and the `verify` are on the job.
    assert!(plan.contains("out/image/sub=01.json"), "{plan}");
    assert!(plan.contains("out/mean/global.nii.gz"), "{plan}");
    assert!(plan.contains("nonempty"), "{plan}");
}

#[test]
fn a_collection_below_its_minimum_is_left_out_when_members_are() {
    let (tree, _) = dataset();
    // Only one complete member remains, below `@ min(2)`.
    std::fs::remove_file(tree.path().join("d/mask/02.nii")).unwrap();
    let recipe = tree.path().join("p.spitin");
    let output = spit(&["dag", recipe.to_str().unwrap(), "--partial", "--counts"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(stdout.contains("   0  mean = avg"), "{stdout}");
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("1 jobs resolved: 1 in pre, 0 in post"),
        "{stderr}"
    );
    assert!(
        stderr.contains("5 artifacts that cannot be produced"),
        "{stderr}"
    );
}
