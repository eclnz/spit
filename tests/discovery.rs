//! Source discovery: matching the files under a root against the sources'
//! path rules to build an inventory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use spit::{discover_source_files, discover_sources, parse_pipeline, resolve};

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str, files: &[&str]) -> Self {
        let root =
            std::env::temp_dir().join(format!("spit-discover-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn spit(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .output()
        .unwrap()
}

const ONE_SOURCE: &str = "source x [s]\npath x: in/{s}.txt\n";

const DISCOVERED: &str = "\
path: derived/{product}/{entities}.txt
source frame [subject, run]
path frame: raw/sub-{subject}/run-{run}.dat
source lut []
path lut: config/lut.txt
operation stack(frames: many Frame, lut: Lut) -> Stack @ drop(run)
command stack: stack {frames} {lut} {output}
stacked = stack(frame @ vary(run), lut)
";

#[test]
fn sources_are_discovered_from_their_path_rules() {
    let tree = Tree::new(
        "discover",
        &[
            "raw/sub-a/run-1.dat",
            "raw/sub-a/run-10.dat",
            "raw/sub-b/run-2.dat",
            "raw/sub-b/notes.txt",
            "config/lut.txt",
            "derived/stacked/subject=a.txt",
        ],
    );
    let pipeline = parse_pipeline(DISCOVERED).unwrap();
    let inventory = discover_sources(&pipeline, &tree.0).unwrap();
    let records: Vec<_> = inventory
        .artifacts
        .iter()
        .map(|record| format!("{}[{}]", record.product, record.entities))
        .collect();
    assert_eq!(
        records,
        [
            "frame[run=1,subject=a]",
            "frame[run=10,subject=a]",
            "frame[run=2,subject=b]",
            "lut[]"
        ]
    );
    assert_eq!(resolve(&pipeline, &inventory).unwrap().jobs.len(), 2);
}

#[test]
fn a_file_matching_two_source_rules_is_rejected() {
    // Distinct rules that both fit `in/q-x.txt`: `a` with id=q-x, `b` with id=q.
    let text = "source a [id]\npath a: in/{id}.txt\nsource b [id]\npath b: in/{id}-x.txt\n";
    let tree = Tree::new("ambiguous", &["in/q-x.txt"]);
    let error = discover_sources(&parse_pipeline(text).unwrap(), &tree.0).unwrap_err();
    assert!(
        error
            .message()
            .contains("matches the path rules of both `a` and `b`"),
        "{error}"
    );
}

#[test]
fn cli_discovers_sources_under_the_root() {
    let tree = Tree::new(
        "cli-discover",
        &[
            "raw/sub-a/run-1.dat",
            "raw/sub-a/run-2.dat",
            "config/lut.txt",
        ],
    );
    let pipeline = tree.0.join("pipeline.spit");
    fs::write(&pipeline, DISCOVERED).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args(args)
            .output()
            .unwrap()
    };
    let root = tree.0.to_str().unwrap();
    let discovered = run(&["discover", pipeline.to_str().unwrap(), "--root", root]);
    assert!(discovered.status.success());
    assert_eq!(
        String::from_utf8(discovered.stdout).unwrap(),
        "sources:\n    frame[subject=a,run=1]\n    frame[subject=a,run=2]\n    lut[]\n"
    );
    let checked = run(&["check", pipeline.to_str().unwrap(), "--root", root]);
    assert!(checked.status.success());
    let report = String::from_utf8(checked.stdout).unwrap();
    assert!(report.contains("1 jobs resolved."), "{report}");
    assert!(String::from_utf8(checked.stderr)
        .unwrap()
        .contains("note: discovered 3 source artifacts"));
}

#[test]
fn discovery_skips_values_spit_would_write_differently() {
    // `%41` decodes to `A`, whose path SPIT writes as `in/A.txt`.
    let tree = Tree::new(
        "canonical",
        &[
            "in/%41.txt",
            "in/%2e.txt",
            "in/x%zz.txt",
            "in/%2E%2E.txt",
            "in/b.txt",
        ],
    );
    let pipeline = parse_pipeline(ONE_SOURCE).unwrap();
    let discovery = discover_source_files(&pipeline, tree.path()).unwrap();
    let records: Vec<_> = discovery
        .inventory
        .artifacts
        .iter()
        .map(|record| record.entities.to_string())
        .collect();
    assert_eq!(records, ["s=..", "s=b"]);
    assert_eq!(discovery.skipped.len(), 3, "{:?}", discovery.skipped);
    assert!(discovery.skipped[0].starts_with("`in/%2e.txt`"));
    assert!(discovery.skipped[1].contains("is not how SPIT writes a value"));
    assert!(discovery.skipped[2].contains("is not valid `%XX` text"));
}

#[test]
fn discovery_reports_skipped_files_and_still_succeeds() {
    let tree = Tree::new("skipped", &["in/%41.txt", "in/a.txt"]);
    let pipeline = tree.path().join("pipeline.spit");
    fs::write(&pipeline, ONE_SOURCE).unwrap();
    let output = spit(&[
        "discover",
        pipeline.to_str().unwrap(),
        "--root",
        tree.path().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "sources:\n    x[s=a]\n");
    assert!(text(&output.stderr).contains("warning: skipped `in/%41.txt`"));
}

#[test]
fn discovery_is_fast_however_values_could_be_split() {
    // Adjacent placeholders, or separators values may hold, give a long
    // file name exponentially many ways to split.
    let long = format!("in/{}.txt", "a".repeat(200));
    let dashed = format!("in/{}z.txt", "a-".repeat(100));
    let tree = Tree::new("backtrack", &[&long, &dashed]);
    for rule in [
        "source x [a, b, c, d, e, f, g]\npath x: in/{a}{b}{c}{d}{e}{f}{g}.dat\n",
        "source x [a, b, c, d, e]\npath x: in/{a}-{b}-{c}-{d}-{e}.dat\n",
    ] {
        let started = Instant::now();
        let discovery = discover_source_files(&parse_pipeline(rule).unwrap(), tree.path()).unwrap();
        assert!(discovery.inventory.artifacts.is_empty());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }
    // A match still finds its values, including a repeated dimension.
    let tree = Tree::new("repeat", &["in/ab-x/ab.txt", "in/ab-x/cd.txt"]);
    let rule = "source x [s, t]\npath x: in/{s}-{t}/{s}.txt\n";
    let discovery = discover_source_files(&parse_pipeline(rule).unwrap(), tree.path()).unwrap();
    let records: Vec<_> = discovery
        .inventory
        .artifacts
        .iter()
        .map(|record| record.entities.to_string())
        .collect();
    assert_eq!(records, ["s=ab,t=x"]);
}

#[cfg(unix)]
#[test]
fn discovery_follows_links_without_looping() {
    let tree = Tree::new("links", &["elsewhere/c.txt", "data/in/a.txt"]);
    let data = tree.path().join("data");
    std::os::unix::fs::symlink(tree.path().join("elsewhere/c.txt"), data.join("in/b.txt")).unwrap();
    std::os::unix::fs::symlink(&data, data.join("in/loop")).unwrap();
    let pipeline = parse_pipeline(ONE_SOURCE).unwrap();
    let discovery = discover_source_files(&pipeline, &data).unwrap();
    let records: Vec<_> = discovery
        .inventory
        .artifacts
        .iter()
        .map(|record| record.entities.to_string())
        .collect();
    assert_eq!(records, ["s=a", "s=b"]);
}
