//! `{x.dir}` and `{x.stem}` give an output's folder and its file name
//! without its extension, for a tool such as dcm2niix that takes a folder
//! and a name and adds the extension itself.

mod support;

use spit::diagnose;
use support::{spit, text, Tree};

const CONVERT: &str = "\
path: derivatives/{product}/{entities}
source dicom [sub]
path dicom: sourcedata/sub-{sub}
operation convert(dicom) -> (image: Image .nii.gz, meta: Json .json beside image)
command convert: dcm2niix -z y -b y -o {image.dir} -f {image.stem} {dicom}
operation strip(t1: Image) -> (brain: Image .nii.gz, mask: Image \"_mask.nii.gz\" beside brain)
command strip: bet {t1} {brain.dir}/{brain.stem} -m --mask-name {mask.stem}
operation flat(dicom) -> .txt
command flat: dump {dicom} --into {output.dir} --name {output.stem}
image, meta = convert(dicom)
brain, mask = strip(image)
listing = flat(dicom)
path listing: {sub}
";

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.message)
        .collect()
}

/// `spit dag` over one subject, with `extra` arguments.
fn plan(extra: &[&str]) -> String {
    let tree = Tree::new("folder-stem", &[]);
    let pipeline = tree.write("pipeline.spit", CONVERT);
    let inputs = tree.write("inputs.spitout", "sources:\n    dicom[sub=01]\n");
    let mut args = vec!["dag", pipeline.to_str().unwrap(), inputs.to_str().unwrap()];
    args.extend(extra);
    let output = spit(&args);
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
}

#[test]
fn a_command_gives_an_outputs_folder_and_name() {
    let commands = plan(&["--commands"]);
    for line in [
        "dcm2niix -z y -b y -o derivatives/image -f sub=01 sourcedata/sub-01",
        // The stem of a beside output drops its own extension.
        "bet derivatives/image/sub=01.nii.gz derivatives/brain/sub=01 -m --mask-name sub=01_mask",
        // A file at the root is in folder `.`.
        "dump sourcedata/sub-01 --into . --name 01",
    ] {
        assert!(commands.contains(line), "{line} in {commands}");
    }
}

#[test]
fn the_plan_names_the_file_each_folder_and_name_is_of() {
    let json = plan(&["--json"]);
    assert!(json.starts_with("{\"version\":4,"), "{json}");
    assert!(
        json.contains(
            "[{\"dir\":\"derivatives/image\",\"of\":\"derivatives/image/sub=01.nii.gz\"}]"
        ),
        "{json}"
    );
    assert!(
        json.contains("[{\"stem\":\"sub=01\",\"of\":\"derivatives/image/sub=01.nii.gz\"}]"),
        "{json}"
    );
    // Parts join into one argument as paths do.
    assert!(
        json.contains("[{\"dir\":\"derivatives/brain\",\"of\":\"derivatives/brain/sub=01.nii.gz\"},\"/\",{\"stem\":\"sub=01\",\"of\":\"derivatives/brain/sub=01.nii.gz\"}]"),
        "{json}"
    );
}

#[test]
fn a_folder_or_name_counts_as_writing_the_output() {
    assert_eq!(errors(CONVERT), Vec::<String>::new());
    assert_eq!(
        errors(&CONVERT.replace("-o {image.dir} -f {image.stem} ", "")),
        ["command for `convert` must use `{image}`"]
    );
}

#[test]
fn folders_and_names_are_checked() {
    for (from, to, message) in [
        (
            "{image.stem}",
            "{image.base}",
            "`{image.base}` gives no `.base`; write `{image}` for the file, `{image.dir}` for its folder, or `{image.stem}` for its name without its extension",
        ),
        (
            "{image.stem}",
            "{dicom.dir}",
            "`{dicom.dir}`: only an operation's outputs give `.dir` and `.stem`",
        ),
        (
            "operation flat(dicom) -> .txt",
            "operation flat(dicom) -> Text",
            "`{output.stem}` is `output`'s file name without its extension, but `flat` declares none for `output`; give it one, as in `-> Image .nii.gz`",
        ),
        (
            "{image.stem}",
            "{picture.stem}",
            "uses unknown placeholder `{picture.stem}`",
        ),
    ] {
        let found = errors(&CONVERT.replace(from, to));
        assert!(
            found.iter().any(|error| error.contains(message)),
            "{to}: {found:?}"
        );
    }
    let verify = errors(&format!("{CONVERT}verify convert: check {{image.dir}}\n"));
    assert!(
        verify
            .iter()
            .any(|error| error.contains("verify for `convert` cannot use output `{image.dir}`")),
        "{verify:?}"
    );
}
