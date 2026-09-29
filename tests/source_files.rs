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

fn check(tree: &Tree) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            "examples/commands/field_survey.spit",
            "examples/commands/field_survey.spitout",
            "--root",
            tree.path().to_str().unwrap(),
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
    let inventory = include_str!("../examples/commands/field_survey.spitout")
        .replace("    photo_imu[site=01,visit=02,shot=02]\n", "");
    let inventory_path = fixture.0.join("incomplete.spitout");
    fs::write(&inventory_path, inventory).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            "examples/commands/field_survey.spit",
            inventory_path.to_str().unwrap(),
            "--root",
            fixture.0.to_str().unwrap(),
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
    let example = |file: &str| format!("{}/examples/commands/{file}", env!("CARGO_MANIFEST_DIR"));
    let result = Command::new(env!("CARGO_BIN_EXE_spit"))
        .current_dir(fixture.path().parent().unwrap())
        .args([
            "dag",
            &example("field_survey.spit"),
            &example("field_survey.spitout"),
            "--root",
            fixture.path().file_name().unwrap().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let graph = String::from_utf8(result.stdout).unwrap();
    let root = fixture.path().to_str().unwrap().replace('\\', "\\\\");
    assert!(graph.contains(&format!("\"root\":\"{root}\"")), "{graph}");
}
