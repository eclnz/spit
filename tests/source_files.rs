//! Check the field survey example against a temporary tree of empty source files.

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

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

struct Fixture(PathBuf);

impl Fixture {
    fn new(missing: Option<&str>) -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "spit mock source tree {} {suffix} {} with spaces",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for relative in survey_sources() {
            if Some(relative.as_str()) == missing {
                continue;
            }
            let path = root.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            File::create(path).unwrap();
        }
        Self(root)
    }

    fn check(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args([
                "check",
                "examples/commands/field_survey.spit",
                "--sources",
                "examples/commands/field_survey.sources",
                "--root",
                self.0.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn survey_compiles_when_all_required_source_files_exist() {
    let fixture = Fixture::new(None);
    let result = fixture.check();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report = String::from_utf8(result.stdout).unwrap();
    assert!(report.contains("93 jobs resolved."));
    assert!(report.contains("39 source files verified."));
    assert!(!fixture.0.join("derivatives").exists());
}

#[test]
fn survey_reports_a_missing_required_file() {
    let missing = "site-01/visit-02/photos/site-01_visit-02_shot-02_photo.gpx";
    let fixture = Fixture::new(Some(missing));
    let result = fixture.check();
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
    let fixture = Fixture::new(None);
    let inventory = include_str!("../examples/commands/field_survey.sources")
        .replace("    photo_imu[site=01,visit=02,shot=02]\n", "");
    let inventory_path = fixture.0.join("incomplete.sources");
    fs::write(&inventory_path, inventory).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            "examples/commands/field_survey.spit",
            "--sources",
            inventory_path.to_str().unwrap(),
            "--root",
            fixture.0.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(
        error.contains("missing input `imu` for `import_photo`"),
        "{error}"
    );
    assert!(error.contains("shot=02,site=01,visit=02"), "{error}");
}

#[test]
fn survey_reports_a_source_path_that_is_a_directory() {
    let directory = "config/target_classes.txt";
    let fixture = Fixture::new(Some(directory));
    fs::create_dir(fixture.0.join(directory)).unwrap();
    let result = fixture.check();
    assert!(!result.status.success());
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("target_classes"));
}
