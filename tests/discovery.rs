//! Source discovery: building an inventory from the files and directories
//! under a root, including named discovery rules, their coverage and skips.

mod support;

use support::{outputs, spit, text, Tree};

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use spit::{
    discover_source_files, discover_sources, parse_pipeline, parse_source_inventory, resolve,
    InputSource, InputSpec, Pipeline, ResolveError, ResolvedInputs, SourceInventory,
};

/// A document's pipeline, and the input rules written beside it.
fn parse(text: &str) -> (Pipeline, InputSpec) {
    let (pipeline, spec, _) = support::parse_with_rules(text).unwrap();
    (pipeline, spec)
}

/// Run the input stage over records already found.
fn settle(pipeline: &Pipeline, spec: &InputSpec, inventory: &SourceInventory) -> ResolvedInputs {
    spec.resolve(pipeline, InputSource::Inventory(inventory.clone()))
        .unwrap()
}

const ONE_SOURCE: &str = "source x [s]\npath x: in/{s}.txt\n";

#[test]
fn unmatched_files_are_counted_and_can_be_listed_without_an_inventory() {
    let tree = Tree::new(
        "unmatched",
        &["data/in/a.txt", "data/in/a.txt.bak", "data/notes.md"],
    );
    tree.write("pipeline.spit", ONE_SOURCE);
    let recipe = tree.write("recipe.spitin", "pipeline pipeline.spit\nroot data\n");
    let recipe = recipe.to_str().unwrap();

    let regular = spit(&["inputs", recipe]);
    assert!(regular.status.success(), "{}", text(&regular.stderr));
    assert!(text(&regular.stdout).contains("x[s=a]"));
    assert!(
        text(&regular.stderr)
            .contains("match no source rule and are not read: `in/a.txt.bak` and `notes.md`"),
        "{}",
        text(&regular.stderr)
    );

    let listing = spit(&["inputs", recipe, "--unmatched"]);
    assert!(listing.status.success(), "{}", text(&listing.stderr));
    assert_eq!(text(&listing.stdout), "in/a.txt.bak\nnotes.md\n");
    assert!(!text(&listing.stdout).contains("sources:"));
}

#[test]
fn many_unmatched_files_are_counted_by_extension() {
    let tree = Tree::new(
        "unmatched-by-extension",
        &[
            "data/in/a.txt",
            "data/in/a.json",
            "data/in/b.json",
            "data/in/c.json",
            "data/notes.md",
            "data/README",
        ],
    );
    tree.write("pipeline.spit", ONE_SOURCE);
    let recipe = tree.write("recipe.spitin", "pipeline pipeline.spit\nroot data\n");
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("note: 5 files under `"));
    assert!(
        text(&output.stderr).contains(
            "/data` match no source rule and are not read \
             (3 `.json`, 1 with no extension, 1 `.md`), such as `README`"
        ),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn files_at_the_pipelines_output_paths_are_not_unmatched() {
    let tree = Tree::new(
        "outputs-not-unmatched",
        &[
            "data/in/a.txt",
            "data/out/clean/cleaned/s=a.csv",
            "data/out/report.csv",
            "data/out/stored/s=a.zarr/0/0",
            "data/out/stored/s=a.zarr/.zattrs",
            "data/out/clean/cleaned/s=a.csv.bak",
        ],
    );
    tree.write(
        "pipeline.spit",
        "source x [s]\npath x: in/{s}.txt\npath: out/{@stage}/{@product}/{@entities}\next: .csv\n\
         path report: out/report.csv\npath stored: out/stored/{@entities}\n\
         stage clean:\n    operation clean(x) -> Clean\n    command clean: clean {x} {@output}\n    cleaned = clean(x)\n\
         operation summarize(items: many Clean) -> Report\ncommand summarize: summarize {items} {@output}\n\
         report = summarize(cleaned @ vary(s))\n\
         operation store(table: Clean) -> .zarr/\ncommand store: store {table} {@output}\nstored = store(cleaned)\n",
    );
    let recipe = tree.write("recipe.spitin", "pipeline pipeline.spit\nroot data\n");
    let recipe = recipe.to_str().unwrap();

    let listing = spit(&["inputs", recipe, "--unmatched"]);
    assert!(listing.status.success(), "{}", text(&listing.stderr));
    assert_eq!(text(&listing.stdout), "out/clean/cleaned/s=a.csv.bak\n");

    let regular = spit(&["inputs", recipe]);
    assert!(regular.status.success(), "{}", text(&regular.stderr));
    assert!(
        text(&regular.stderr).contains("1 file under"),
        "{}",
        text(&regular.stderr)
    );
}

const DISCOVERED: &str = "\
path: derived/{@product}/{@entities}.txt
source frame [subject, run]
path frame: raw/sub-{subject}/run-{run}.dat
source lut
path lut: config/lut.txt
operation stack(frames: many Frame, lut: Lut) -> Stack
command stack: stack {frames} {lut} {@output}
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
    let (pipeline, spec) = parse(DISCOVERED);
    let inventory = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap();
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
fn directory_discovery_finds_observed_subject_session_pairs() {
    let tree = Tree::new(
        "directory-contexts",
        &[
            "data/sub-A/ses-baseline/image.nii.gz",
            "data/sub-pilot-X/ses-followup/image.nii.gz",
            "data/sub-ignored/other/file.txt",
            "data/sub-Z/ses-visit-10/image.nii.gz",
        ],
    );
    let text = "\
discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
path: results/{@product}/{@entities}.txt
source image [sub, ses]
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
operation process(image) -> Output
output = process(image)
";
    let (pipeline, spec) = parse(text);
    let inventory = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap();
    let contexts: Vec<_> = inventory.contexts.iter().map(ToString::to_string).collect();
    assert_eq!(
        contexts,
        [
            "ses=baseline,sub=A",
            "ses=followup,sub=pilot-X",
            "ses=visit-10,sub=Z",
        ]
    );
    assert_eq!(inventory.artifacts.len(), 3);
    assert_eq!(resolve(&pipeline, &inventory).unwrap().jobs.len(), 3);
}

#[test]
fn discovered_directories_require_source_files_even_without_coverage_rules() {
    let tree = Tree::new("directory-coverage", &["data/sub-A/ses-1/image.nii.gz"]);
    fs::create_dir_all(tree.0.join("data/sub-B/ses-followup")).unwrap();
    let text = "\
discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
source image [sub, ses]
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
";
    let (pipeline, spec) = parse(text);
    let error = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap_err();
    assert!(
        error
            .message()
            .contains("missing source file for `image[ses=followup,sub=B]`"),
        "{error}"
    );
}

#[test]
fn directory_bindings_expand_sources_at_their_declared_dimensions() {
    let tree = Tree::new(
        "directory-projection",
        &[
            "data/sub-A/ses-1/image.nii.gz",
            "data/sub-A/ses-2/image.nii.gz",
            "data/sub-B/ses-baseline/image.nii.gz",
            "data/sub-A/reference.nii.gz",
            "data/sub-B/reference.nii.gz",
        ],
    );
    let (pipeline, spec) = parse(
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         source image [sub, ses]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n\
         source reference [sub]\n\
         path reference: data/sub-{sub}/reference.nii.gz\n",
    );
    let inventory = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap();
    let references: Vec<_> = inventory
        .artifacts
        .iter()
        .filter(|record| record.product == "reference")
        .map(|record| record.entities.get("sub").unwrap())
        .collect();
    assert_eq!(inventory.artifacts.len(), 5);
    assert_eq!(references, ["A", "B"]);
}

#[test]
fn coverage_can_target_the_named_discovery_rule() {
    let tree = Tree::new(
        "directory-require",
        &[
            "data/sub-1/ses-1/.keep",
            "data/sub-1/ses-2/.keep",
            "data/sub-2/ses-1/.keep",
            "data/sub-2/ses-2/.keep",
            "data/sub-2/ses-3/.keep",
            "data/sub-3/ses-1/.keep",
            "data/sub-3/ses-2/.keep",
            "data/sub-3/ses-4/.keep",
            "data/sub-5/ses-1/.keep",
        ],
    );
    let text = "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
                require sessions count>=2 per [sub]\n";
    let (pipeline, spec) = parse(text);
    let inventory = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap();
    assert_eq!(inventory.discovered["sessions"].len(), 9);
    let rendered = spit::render_source_inventory(&inventory, &pipeline, &spec.rules);
    assert!(rendered.starts_with("contexts sessions:\n"), "{rendered}");
    assert_eq!(parse_source_inventory(&rendered).unwrap(), inventory);
    let inline = format!("{text}{rendered}");
    let (inline_pipeline, inline_spec, inline_inventory) =
        support::parse_with_rules(&inline).unwrap();
    let inline_inventory = inline_inventory.expect("named contexts");
    assert!(matches!(
        settle(&inline_pipeline, &inline_spec, &inline_inventory).require_complete(),
        Err(ResolveError::CoverageViolation {
            found: 1,
            discovery: true,
            ..
        })
    ));
    assert!(matches!(
        settle(&pipeline, &spec, &inventory).require_complete(),
        Err(ResolveError::CoverageViolation { product, context, found: 1, discovery: true, .. })
            if product == "sessions" && context.get("sub") == Some("5")
    ));
    let values = text.replace("count>=2", "ses=1,2");
    let (pipeline, spec) = parse(&values);
    assert!(matches!(
        settle(&pipeline, &spec, &inventory).require_complete(),
        Err(ResolveError::MissingRequiredValue { product, context, dimension, value, discovery: true, .. })
            if product == "sessions" && context.get("sub") == Some("5") && dimension == "ses" && value == "2"
    ));
}

#[test]
fn drop_discovery_group_removes_subject_before_source_checks_and_jobs() {
    let tree = Tree::new(
        "directory-drop",
        &[
            "data/sub-1/ses-1/image.nii.gz",
            "data/sub-1/ses-2/image.nii.gz",
            "data/sub-2/ses-1/image.nii.gz",
            "data/sub-2/ses-2/image.nii.gz",
            "data/sub-2/ses-3/image.nii.gz",
            "data/sub-3/ses-1/image.nii.gz",
            "data/sub-3/ses-2/image.nii.gz",
            "data/sub-3/ses-4/image.nii.gz",
            "data/sub-5/ses-1/.keep",
        ],
    );
    let text = "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
                drop [sub] where sessions count<2\n\
                require sessions count>=2 per [sub]\n\
                source image [sub, ses]\n\
                path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n\
                operation process(image: Image) -> Image\n\
                result = process(image)\n\
                path result: out/sub-{sub}/ses-{ses}/result.nii.gz\n";
    let (pipeline, spec) = parse(text);
    let inventory = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap();
    assert_eq!(inventory.discovered["sessions"].len(), 8);
    assert_eq!(inventory.artifacts.len(), 8);
    assert!(!inventory
        .contexts
        .iter()
        .any(|binding| binding.get("sub") == Some("5")));
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 8);
    assert!(!outputs(&dag).iter().any(|output| output.contains("sub=5")));

    let pairs = [
        ("1", "1"),
        ("1", "2"),
        ("2", "1"),
        ("2", "2"),
        ("2", "3"),
        ("3", "1"),
        ("3", "2"),
        ("3", "4"),
        ("5", "1"),
    ];
    let mut explicit = String::from("contexts sessions:\n");
    for (sub, ses) in pairs {
        explicit.push_str(&format!("[sub={sub},ses={ses}]\n"));
    }
    explicit.push_str("sources:\n");
    for (sub, ses) in pairs {
        explicit.push_str(&format!("image[sub={sub},ses={ses}]\n"));
    }
    let explicit = parse_source_inventory(&explicit).unwrap();
    let settled = settle(&pipeline, &spec, &explicit);
    let dag = resolve(&pipeline, &settled.dag_inventory()).unwrap();
    assert_eq!(dag.jobs.len(), 8);
    assert!(!outputs(&dag).iter().any(|output| output.contains("sub=5")));
}

#[test]
fn drop_source_group_can_omit_missing_files_in_a_discovered_context() {
    let tree = Tree::new(
        "source-drop",
        &[
            "data/sub-1/ses-1/image.nii.gz",
            "data/sub-1/ses-2/image.nii.gz",
            "data/sub-5/ses-1/image.nii.gz",
            "data/sub-5/ses-2/.keep",
        ],
    );
    let (pipeline, spec) = parse(
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         source image [sub, ses]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n\
         drop [sub] where image count<2\n\
         operation process(image: Image) -> Image\n\
         result = process(image)\n",
    );
    let inventory = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap();
    assert_eq!(inventory.artifacts.len(), 2);
    assert!(inventory
        .contexts
        .iter()
        .all(|binding| binding.get("sub") == Some("1")));
    assert_eq!(resolve(&pipeline, &inventory).unwrap().jobs.len(), 2);
}

#[test]
fn drop_does_not_hide_invalid_inventory_bindings() {
    let (pipeline, spec) = parse(
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         drop [sub] where sessions count<2\n",
    );
    let inventory = parse_source_inventory("contexts sessions:\n[sub=5]\n").unwrap();
    let error = spec
        .resolve(&pipeline, InputSource::Inventory(inventory))
        .unwrap_err();
    assert!(
        error.to_string().contains("must bind [sub, ses]"),
        "{error}"
    );
}

#[test]
fn discovery_coverage_uses_only_its_own_bindings() {
    let tree = Tree::new(
        "directory-rule-scope",
        &[
            "data/sub-A/ses-1/.keep",
            "controls/sub-A/ses-1/.keep",
            "controls/sub-A/ses-2/.keep",
        ],
    );
    let (pipeline, spec) = parse(
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         discover controls: [sub, ses] from dirs controls/sub-{sub}/ses-{ses}\n\
         require sessions count>=2 per [sub]\n",
    );
    let inventory = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap();
    assert_eq!(inventory.contexts.len(), 2);
    assert_eq!(inventory.discovered["sessions"].len(), 1);
    assert_eq!(inventory.discovered["controls"].len(), 2);
    assert!(matches!(
        settle(&pipeline, &spec, &inventory).require_complete(),
        Err(ResolveError::CoverageViolation {
            found: 1,
            discovery: true,
            ..
        })
    ));
}

#[test]
fn directory_discovery_rejects_unsafe_or_incomplete_patterns() {
    for declaration in [
        "discover sessions: [sub, ses] from dirs data/sub-{sub}",
        "discover sessions: [sub] from dirs ../data/sub-{sub}",
        "discover sessions: [sub] from dirs data/sub-{other}",
        "discover sessions: [sub, sub] from dirs data/sub-{sub}",
        "discover sessions [sub] from dirs data/sub-{sub}",
    ] {
        assert!(parse_pipeline(declaration).is_err(), "{declaration}");
    }
}

#[test]
fn directory_discovery_errors_when_no_directories_match() {
    let tree = Tree::new("directory-empty", &["data/unrelated/file.txt"]);
    let (pipeline, spec) =
        parse("discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n");
    let error = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap_err();
    assert!(
        error
            .message()
            .contains("discovery `sessions` matched no directories"),
        "{error}"
    );
    assert!(
        error.message().contains("data/sub-{sub}/ses-{ses}"),
        "{error}"
    );
}

#[test]
fn a_file_matching_two_source_rules_is_rejected() {
    // Distinct rules that both fit `in/q-x.txt`: `a` with id=q-x, `b` with id=q.
    let text = "source a [id]\npath a: in/{id}.txt\nsource b [id]\npath b: in/{id}-x.txt\n";
    let tree = Tree::new("ambiguous", &["in/q-x.txt"]);
    let (pipeline, spec) = parse(text);
    let error = discover_sources(&pipeline, &spec.rules, &tree.0).unwrap_err();
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
    // A recipe with no rules: sources are found by the pipeline's path rules.
    let recipe = tree.0.join("dataset.spitin");
    fs::write(&recipe, "pipeline pipeline.spit\nroot .\n").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args(args)
            .current_dir(&tree.0)
            .output()
            .unwrap()
    };
    let discovered = run(&["inputs", recipe.to_str().unwrap()]);
    assert!(discovered.status.success());
    // A printed .spitout records no root: where it will be kept is unknown.
    assert_eq!(
        String::from_utf8_lossy(&discovered.stdout),
        "sources:\n    frame[subject=a,run=1]\n    frame[subject=a,run=2]\n    lut\n"
    );
    assert!(String::from_utf8_lossy(&discovered.stderr).contains("note: found 3 source artifacts"));
    // One written beside the data records it as its root.
    let spitout = tree.0.join("found.spitout");
    let written = run(&["inputs", recipe.to_str().unwrap(), "-o", "found.spitout"]);
    assert!(written.status.success());
    assert!(fs::read_to_string(&spitout)
        .unwrap()
        .starts_with("root .\n"));
    let dag = run(&["dag", pipeline.to_str().unwrap(), spitout.to_str().unwrap()]);
    let notes = String::from_utf8(dag.stderr).unwrap();
    assert!(dag.status.success(), "{notes}");
    assert!(notes.contains("note: 1 jobs resolved."), "{notes}");
    assert!(notes.contains("note: 3 source files verified."), "{notes}");
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
    let (pipeline, spec) = parse(ONE_SOURCE);
    let discovery = discover_source_files(&pipeline, &spec.rules, tree.path()).unwrap();
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
    fs::write(tree.path().join("pipeline.spit"), ONE_SOURCE).unwrap();
    let recipe = tree.path().join("dataset.spitin");
    fs::write(&recipe, "pipeline pipeline.spit\nroot .\n").unwrap();
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
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
        let (pipeline, spec) = parse(rule);
        let discovery = discover_source_files(&pipeline, &spec.rules, tree.path()).unwrap();
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
    let (pipeline, spec) = parse(rule);
    let discovery = discover_source_files(&pipeline, &spec.rules, tree.path()).unwrap();
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
    let (pipeline, spec) = parse(ONE_SOURCE);
    let discovery = discover_source_files(&pipeline, &spec.rules, &data).unwrap();
    let records: Vec<_> = discovery
        .inventory
        .artifacts
        .iter()
        .map(|record| record.entities.to_string())
        .collect();
    assert_eq!(records, ["s=a", "s=b"]);
}
