//! Folder artifacts: a source or output declared with a `/` after its type
//! is a folder, for a tool that reads or writes a folder of files.

mod support;

use spit::diagnose;
use support::{spit, text, Tree};

/// A DICOM series per subject, converted, reconstructed into a folder, and
/// summarised from it.
const RECON: &str = "\
source dicom : Dicom / [sub]
path dicom: dicom/sub={sub}
operation convert(dicom: Dicom) -> Image .nii.gz
command convert: dcm2niix -o {@output.dir} -f {@output.stem} {dicom}
image = convert(dicom)
operation recon(t1: Image) -> (subject: FsSubject /)
command recon: recon-all -i {t1} -sd {subject.dir} -s {subject.stem}
subject = recon(image)
operation stats(subject: FsSubject) -> Table .csv
command stats: summarise {subject} {@output}
table = stats(subject)
";

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.message)
        .collect()
}

/// A dataset with two subjects' DICOM folders, and the pipeline beside it.
fn dataset(pipeline: &str) -> Tree {
    let tree = Tree::new(
        "folders",
        &[
            "data/dicom/sub=01/1.dcm",
            "data/dicom/sub=01/2.dcm",
            "data/dicom/sub=02/1.dcm",
        ],
    );
    tree.write("pipeline.spit", pipeline);
    tree
}

/// `spit` with `args`, then the pipeline and `--root data`, from `tree`.
fn run(tree: &Tree, args: &[&str]) -> std::process::Output {
    let pipeline = tree.path().join("pipeline.spit");
    let root = tree.path().join("data");
    let mut all = args.to_vec();
    all.extend([pipeline.to_str().unwrap(), "--root", root.to_str().unwrap()]);
    spit(&all)
}

#[test]
fn a_folder_source_is_found_and_its_files_are_not_unmatched() {
    let tree = dataset(RECON);
    let found = run(&tree, &["inputs"]);
    assert!(found.status.success(), "{}", text(&found.stderr));
    let stdout = text(&found.stdout);
    assert!(stdout.contains("dicom[sub=01]"), "{stdout}");
    assert!(stdout.contains("dicom[sub=02]"), "{stdout}");
    // The `.dcm` files are read with their folders.
    assert!(!text(&found.stderr).contains("match no source rule"));
}

#[test]
fn a_folder_is_given_to_a_command_by_its_path() {
    let tree = dataset(RECON);
    let plan = run(&tree, &["dag", "--commands"]);
    assert!(plan.status.success(), "{}", text(&plan.stderr));
    let commands = text(&plan.stdout);
    for line in [
        "dcm2niix -o out/image -f sub=01 dicom/sub=01",
        // A folder without an extension: its stem is its whole name.
        "recon-all -i out/image/sub=01.nii.gz -sd out/subject -s sub=01",
        "summarise out/subject/sub=01 out/table/sub=01.csv",
    ] {
        assert!(commands.contains(line), "{line} in {commands}");
    }
}

#[test]
fn the_plan_marks_each_folder() {
    let tree = dataset(RECON);
    let paths = run(&tree, &["dag", "--paths"]);
    assert!(paths.status.success(), "{}", text(&paths.stderr));
    let paths = text(&paths.stdout);
    assert!(paths.contains("path: dicom/sub=01/\n"), "{paths}");
    assert!(paths.contains("path: out/subject/sub=01/\n"), "{paths}");
    assert!(paths.contains("path: out/image/sub=01.nii.gz\n"), "{paths}");

    let json = run(&tree, &["dag", "--json"]);
    let json = text(&json.stdout);
    assert!(json.starts_with("{\"version\":5,"), "{json}");
    assert!(
        json.contains("\"path\":\"dicom/sub=01\",\"kind\":\"folder\"}"),
        "{json}"
    );
    assert!(
        json.contains("\"path\":\"out/subject/sub=01\",\"kind\":\"folder\"}"),
        "{json}"
    );
    assert!(
        json.contains("\"path\":\"out/table/sub=01.csv\",\"kind\":\"file\"}"),
        "{json}"
    );
}

#[test]
fn hovers_and_path_rules_show_a_folder() {
    let tree = dataset(RECON);
    let pipeline = tree.path().join("pipeline.spit");
    let pipeline = pipeline.to_str().unwrap();
    let rules = spit(&["check", pipeline, "--path-rules"]);
    let rules = text(&rules.stdout);
    assert!(
        rules.contains("  dicom (source folder): explicit dicom/sub={sub}"),
        "{rules}"
    );
    assert!(
        rules.contains("  subject (output folder): built-in default"),
        "{rules}"
    );

    let hovers = spit(&["check", pipeline, "--json", "--hovers"]);
    let hovers = text(&hovers.stdout);
    assert!(
        hovers.contains("\"signature\":\"dicom: Dicom / [sub]\""),
        "{hovers}"
    );
    assert!(hovers.contains("-> (subject: FsSubject /)"), "{hovers}");
}

#[test]
fn a_folder_may_have_an_extension_and_takes_no_default_one() {
    let tree = Tree::new("folder-extension", &[]);
    let pipeline = tree.write(
        "pipeline.spit",
        "\
ext: .txt
source table [id]
path table: in/{id}.csv
operation store(table) -> Zarr .zarr/
command store: to_zarr {table} {@output.dir} {@output.stem}
operation index(table) -> Index /
command index: build_index {table} {@output}
stored = store(table)
indexed = index(table)
",
    );
    let inputs = tree.write("inputs.spitout", "sources:\n    table[id=a]\n");
    let plan = spit(&[
        "dag",
        pipeline.to_str().unwrap(),
        inputs.to_str().unwrap(),
        "--commands",
    ]);
    assert!(plan.status.success(), "{}", text(&plan.stderr));
    let commands = text(&plan.stdout);
    assert!(
        commands.contains("to_zarr in/a.csv out/stored id=a\n"),
        "{commands}"
    );
    // `ext:` gives files their extension, not folders.
    assert!(
        commands.contains("build_index in/a.csv out/indexed/id=a\n"),
        "{commands}"
    );
}

#[test]
fn a_path_of_the_wrong_kind_is_named() {
    let tree = dataset(RECON);
    tree.write("data/dicom/sub=03", "");
    let found = run(&tree, &["inputs"]);
    assert!(found.status.success(), "{}", text(&found.stderr));
    assert!(
        text(&found.stderr).contains(
            "warning: skipped `dicom/sub=03`: is a file, but source `dicom` reads folders"
        ),
        "{}",
        text(&found.stderr)
    );

    let files = dataset(&RECON.replace("Dicom / [sub]", "Dicom [sub]"));
    let found = run(&files, &["inputs"]);
    assert!(
        text(&found.stderr).contains(
            "warning: skipped `dicom/sub=01`: is a folder, but source `dicom` reads files; end its declaration with `/` to read folders"
        ),
        "{}",
        text(&found.stderr)
    );
}

#[test]
fn a_missing_source_folder_is_an_error() {
    let tree = dataset(RECON);
    let pipeline = tree.path().join("pipeline.spit");
    // The record names a folder that is a file on disk.
    tree.write("data/dicom/sub=03", "");
    let inputs = tree.write("inputs.spitout", "root data\nsources:\n    dicom[sub=03]\n");
    let plan = spit(&["dag", pipeline.to_str().unwrap(), inputs.to_str().unwrap()]);
    assert!(!plan.status.success());
    assert!(
        text(&plan.stderr).contains("missing source folder for `dicom[sub=03]`"),
        "{}",
        text(&plan.stderr)
    );
}

#[test]
fn a_discovered_context_needs_its_folder() {
    let tree = dataset(RECON);
    std::fs::create_dir_all(tree.path().join("data/contexts/sub=03")).unwrap();
    for sub in ["01", "02"] {
        std::fs::create_dir_all(tree.path().join(format!("data/contexts/sub={sub}"))).unwrap();
    }
    let recipe = tree.write(
        "recipe.spitin",
        "pipeline pipeline.spit\nroot data\ndiscover subjects: [sub] from dirs contexts/sub={sub}\n",
    );
    let found = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(!found.status.success());
    assert!(
        text(&found.stderr).contains("missing source folder for `dicom[sub=03]`"),
        "{}",
        text(&found.stderr)
    );
}

#[test]
fn a_source_may_sit_in_a_source_folder() {
    let pipeline = "\
source dicom : Dicom / [sub]
path dicom: dicom/sub={sub}
source info : Json [sub]
path info: dicom/sub={sub}/info.json
operation convert(dicom: Dicom, info: Json) -> Image .nii.gz
command convert: convert {dicom} {info} {@output}
image = convert(dicom, info)
";
    assert_eq!(errors(pipeline), Vec::<String>::new());
    let tree = dataset(pipeline);
    tree.write("data/dicom/sub=01/info.json", "{}");
    tree.write("data/dicom/sub=02/info.json", "{}");
    let plan = run(&tree, &["dag", "--commands"]);
    assert!(plan.status.success(), "{}", text(&plan.stderr));
    assert!(text(&plan.stdout).contains("convert dicom/sub=01 dicom/sub=01/info.json"));
}

#[test]
fn nothing_is_written_in_a_folder_or_read_from_one_a_job_writes() {
    let base = "\
source dicom : Dicom / [sub]
path dicom: dicom/sub={sub}
operation convert(dicom: Dicom) -> Image .nii.gz
command convert: convert {dicom} {@output}
image = convert(dicom)
";
    let inside_source = format!("{base}path image: dicom/sub={{sub}}/image.nii.gz\n");
    assert_eq!(
        errors(&inside_source),
        ["path rule for `image` puts files inside `dicom/sub=sub`, the path of a `dicom` source folder, which no job may write inside, for the same entities; distinguish their path rules"]
    );
    let folders = "\
source t1 : Image [sub]
path t1: in/{sub}.nii
operation recon(t1: Image) -> FsSubject /
command recon: recon {t1} {@output}
subject = recon(t1)
path subject: subjects/{sub}
operation stats(subject: FsSubject) -> Table .csv
command stats: stats {subject} {@output}
table = stats(subject)
path table: subjects/{sub}/table.csv
";
    assert_eq!(
        errors(folders),
        ["path rule for `table` puts files inside `subjects/sub`, the path of a `subject` folder, which its job writes whole, for the same entities; distinguish their path rules"]
    );
}

#[test]
fn a_folder_is_never_beside_another_output_nor_a_sidecar() {
    assert_eq!(
        errors("operation c(d) -> (a: X .zarr/, b: J .json beside a)\n"),
        ["`b` is written beside `a`, which is a folder, and only a file has files beside it; drop `beside a` and give the tool `{b}`"]
    );
    assert_eq!(
        errors("operation c(d) -> (a: X .nii, b: J .json/ beside a)\n"),
        ["an output written beside another is a file, not a folder; drop the `/`"]
    );
    assert_eq!(
        errors("sidecars p [s]:\n    source a .raw/\n    source b .json\n"),
        ["a source in sidecars group `p` is a file beside the others, not a folder; drop the `/`"]
    );
}
