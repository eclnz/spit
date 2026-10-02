//! Check the field survey example against a temporary tree of empty source files.

mod support;

use std::fs;
use std::process::{Command, Output};

use support::Tree;

// Construct expected filenames independently of SPIT's path binder.
fn survey_sources() -> Vec<String> {
    let mut paths = Vec::new();
    for (site, visit, shots) in [
        ("01", "01", &["01", "02"][..]),
        ("01", "02", &["01", "02"][..]),
        ("02", "01", &["01", "02", "03"][..]),
    ] {
        let folder = format!("site-{site}/visit-{visit}");
        for shot in shots {
            let photo = format!("{folder}/photos/site-{site}_visit-{visit}_shot-{shot}_photo");
            for extension in ["raw", "gpx", "imu", "json"] {
                paths.push(format!("{photo}.{extension}"));
            }
        }
        let flat = format!("{folder}/calibration/site-{site}_visit-{visit}_flat");
        paths.push(format!("{flat}.raw"));
        paths.push(format!("{flat}.json"));
        paths.push(format!("{folder}/map/site-{site}_visit-{visit}_map.tif"));
    }
    paths.push("config/source_classes.txt".to_owned());
    paths.push("config/target_classes.txt".to_owned());
    paths
}

/// The survey's source files, but `missing`, in a folder whose name has
/// spaces.
fn survey_tree(missing: Option<&str>) -> Tree {
    let files = survey_sources();
    let files: Vec<_> = files
        .iter()
        .map(String::as_str)
        .filter(|file| Some(*file) != missing)
        .collect();
    Tree::new("mock source tree with spaces", &files)
}

const SURVEY: &str = include_str!("../examples/commands/field_survey/field_survey.spitout");

/// `records`, a copy of the survey's `.spitout`, written in `tree` with the
/// tree as its root.
fn inventory_in(tree: &Tree, records: &str) -> std::path::PathBuf {
    let records: String = records
        .lines()
        .map(|line| {
            if line.starts_with("root ") {
                "root ."
            } else {
                line
            }
        })
        .map(|line| format!("{line}\n"))
        .collect();
    tree.write("field_survey.spitout", &records)
}

fn check(tree: &Tree) -> Output {
    let inventory = inventory_in(tree, SURVEY);
    Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            "examples/commands/field_survey/field_survey.spit",
            inventory.to_str().unwrap(),
        ])
        .output()
        .unwrap()
}

#[test]
fn survey_compiles_when_all_required_source_files_exist() {
    let fixture = survey_tree(None);
    let result = check(&fixture);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report = String::from_utf8(result.stderr).unwrap();
    assert!(report.contains("note: 93 jobs resolved."), "{report}");
    assert!(
        report.contains("note: 39 source files verified."),
        "{report}"
    );
    assert!(!fixture.0.join("derivatives").exists());
}

#[test]
fn survey_reports_a_missing_required_file() {
    let missing = "site-01/visit-02/photos/site-01_visit-02_shot-02_photo.gpx";
    let fixture = survey_tree(Some(missing));
    let result = check(&fixture);
    assert!(!result.status.success());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(
        error.contains("missing source file for `photo_gps"),
        "{error}"
    );
    assert!(error.contains(missing));
}

#[test]
fn survey_rejects_a_photo_without_an_inventory_sidecar() {
    let fixture = survey_tree(None);
    let inventory = inventory_in(
        &fixture,
        &SURVEY.replace("    photo_imu[site=01,visit=02,shot=02]\n", ""),
    );
    let result = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            "examples/commands/field_survey/field_survey.spit",
            inventory.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(
        error.contains("no `photo_imu` artifact for input `imu` of `import_photo`"),
        "{error}"
    );
    assert!(error.contains("shot=02,site=01,visit=02"), "{error}");
}

#[test]
fn survey_reports_a_source_path_that_is_a_directory() {
    let directory = "config/target_classes.txt";
    let fixture = survey_tree(Some(directory));
    fs::create_dir(fixture.0.join(directory)).unwrap();
    let result = check(&fixture);
    assert!(!result.status.success());
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("target_classes"));
}

#[test]
fn the_spitdag_records_the_root_as_an_absolute_path() {
    let fixture = survey_tree(None);
    inventory_in(&fixture, SURVEY);
    let pipeline = format!(
        "{}/examples/commands/field_survey/field_survey.spit",
        env!("CARGO_MANIFEST_DIR")
    );
    // The `.spitout` is given relative to the working folder, and its root
    // relative to the `.spitout`.
    let inventory =
        std::path::Path::new(fixture.path().file_name().unwrap()).join("field_survey.spitout");
    let result = Command::new(env!("CARGO_BIN_EXE_spit"))
        .current_dir(fixture.path().parent().unwrap())
        .args(["dag", &pipeline, inventory.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let graph = String::from_utf8(result.stdout).unwrap();
    let root = fixture
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .replace('\\', "\\\\");
    assert!(graph.contains(&format!("\"root\":\"{root}\"")), "{graph}");
}
