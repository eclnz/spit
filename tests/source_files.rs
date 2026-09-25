//! Check the ACT example against a temporary tree of empty source files.

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

// Construct expected BIDS filenames independently of SPIT's path binder.
fn act_sources() -> Vec<String> {
    let mut paths = Vec::new();
    for (sub, ses, runs) in [
        ("01", "01", &["01", "02"][..]),
        ("01", "02", &["01", "02"][..]),
        ("02", "01", &["01", "02", "03"][..]),
    ] {
        let session = format!("sub-{sub}/ses-{ses}");
        for run in runs {
            let dwi = format!("{session}/dwi/sub-{sub}_ses-{ses}_run-{run}_dwi");
            for extension in ["nii.gz", "bvec", "bval", "json"] {
                paths.push(format!("{dwi}.{extension}"));
            }
        }
        let epi = format!("{session}/fmap/sub-{sub}_ses-{ses}_dir-PA_epi");
        paths.push(format!("{epi}.nii.gz"));
        paths.push(format!("{epi}.json"));
        paths.push(format!("{session}/anat/sub-{sub}_ses-{ses}_T1w.nii.gz"));
    }
    paths.push("config/source_lut.txt".to_owned());
    paths.push("config/target_lut.txt".to_owned());
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
        for relative in act_sources() {
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
                "examples/commands/mrtrix3_act.spit",
                "--sources",
                "examples/commands/mrtrix3_act.sources",
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
fn act_pipeline_compiles_when_all_required_source_files_exist() {
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
fn act_pipeline_reports_a_missing_required_file() {
    let missing = "sub-01/ses-02/dwi/sub-01_ses-02_run-02_dwi.bvec";
    let fixture = Fixture::new(Some(missing));
    let result = fixture.check();
    assert!(!result.status.success());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(
        error.contains("missing source file for `dwi_bvec"),
        "{error}"
    );
    assert!(error.contains(missing));
}

#[test]
fn act_pipeline_rejects_a_scan_without_an_inventory_sidecar() {
    let fixture = Fixture::new(None);
    let inventory = include_str!("../examples/commands/mrtrix3_act.sources")
        .replace("    dwi_bval[sub=01,ses=02,run=02]\n", "");
    let inventory_path = fixture.0.join("incomplete.sources");
    fs::write(&inventory_path, inventory).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            "examples/commands/mrtrix3_act.spit",
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
        error.contains("missing input `bval` for `import_dwi`"),
        "{error}"
    );
    assert!(error.contains("run=02,ses=02,sub=01"), "{error}");
}

#[test]
fn act_pipeline_reports_a_source_path_that_is_a_directory() {
    let directory = "config/target_lut.txt";
    let fixture = Fixture::new(Some(directory));
    fs::create_dir(fixture.0.join(directory)).unwrap();
    let result = fixture.check();
    assert!(!result.status.success());
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("target_lut"));
}
