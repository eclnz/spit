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

/// A scan of a dataset and the records of that same scan, with the same
/// `exclude`, `drop` and `require` rules, reach the input stage by two
/// paths, and give the same `.spitdag` and notes.
#[test]
fn a_scan_and_its_records_settle_to_the_same_plan() {
    let mut files = vec!["d/sub-1/ses-2/run-1.nii", "d/sub-1/ses-2/t1w.nii"];
    let mut tree_files = Vec::new();
    for sub in 1..=3 {
        for run in 1..=2 {
            tree_files.push(format!("d/sub-{sub}/ses-1/run-{run}.nii"));
        }
    }
    // Subject 3 has no T1w, so the `drop` removes it.
    tree_files.extend(["d/sub-1/ses-1/t1w.nii", "d/sub-2/ses-1/t1w.nii"].map(String::from));
    files.extend(tree_files.iter().map(String::as_str));
    let tree = Tree::new("settle-twice", &files);
    tree.write(
        "p.spit",
        "source bold : Bold [sub, ses, run]\npath bold: sub-{sub}/ses-{ses}/run-{run}.nii\n\
         source t1w : T1 [sub, ses]\npath t1w: sub-{sub}/ses-{ses}/t1w.nii\n\
         operation align(bold: Bold, t1w: T1) -> Bold\ncommand align: align {bold} {t1w} {@output}\n\
         path: out/{@product}/{@entities}\naligned = align(bold, t1w)\n",
    );
    let rules = "exclude bold[sub=2,ses=1,run=2]   # bad\n\
                 drop [sub] where t1w count<1\n\
                 require [sub, ses] where bold count>=1\n";
    let header = "pipeline p.spit\nroot d\n";
    let base = tree.write("base.spitin", header);
    let raw = tree.path().join("raw.spitout");
    let output = spit(&[
        "inputs",
        base.to_str().unwrap(),
        "-o",
        raw.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let raw = std::fs::read_to_string(raw).unwrap();
    let records = &raw[raw.find("sources:").unwrap()..];
    let scan = tree.write("scan.spitin", &format!("{header}{rules}"));
    let from_records = tree.write("records.spitin", &format!("{header}{rules}\n{records}"));
    let run = |recipe: &std::path::Path| {
        let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
        assert!(output.status.success(), "{}", text(&output.stderr));
        // What was removed is told in notes; which files were found is not
        // the same sentence for a scan and for records.
        let notes: Vec<_> = text(&output.stderr)
            .lines()
            .filter(|line| line.contains("excluded") || line.contains("dropped"))
            .map(String::from)
            .collect();
        (text(&output.stdout), notes)
    };
    let (scanned, scan_notes) = run(&scan);
    let (recorded, record_notes) = run(&from_records);
    assert_eq!(scanned, recorded);
    assert_eq!(scan_notes, record_notes);
    assert!(
        scan_notes
            .iter()
            .any(|note| note.contains("dropped [sub=3]")),
        "{scan_notes:?}"
    );
    assert!(scan_notes
        .iter()
        .any(|note| note.contains("excluded bold[sub=2,ses=1,run=2]")));
}
