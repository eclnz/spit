//! Selectors, broadcasts, mixed cardinality, multiple outputs, collection contracts,
//! coverage values, empty steps, inventory override, and source discovery.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use spit::{
    diagnose, discover_sources, parse_document, parse_pipeline, parse_source_inventory,
    render_bash, render_bound_dag, render_dag, resolve, validate_commands, Diagnostic,
    ResolveError, ResolvedDag, Severity,
};

fn resolve_text(text: &str, inventory: &str) -> Result<ResolvedDag, ResolveError> {
    let pipeline = parse_pipeline(text).unwrap();
    resolve(&pipeline, &parse_source_inventory(inventory).unwrap())
}

fn outputs(dag: &ResolvedDag) -> Vec<String> {
    dag.jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .map(ToString::to_string)
        .collect()
}

fn warnings(diagnostics: Vec<Diagnostic>) -> Vec<String> {
    diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .map(|diagnostic| diagnostic.to_string())
        .collect()
}

const COMBINE: &str = "\
source result [site, run]
source policy [site]
operation combine(results: many Result, policy: Policy) -> Summary @ drop(run)
command combine: summarize {results} --policy {policy} {output}
summary = combine(result @ vary(run), policy)
";

#[test]
fn a_many_input_can_share_an_operation_with_single_inputs() {
    let dag = resolve_text(
        COMBINE,
        "sources:\n  result[site=A,run=1]\n  result[site=A,run=2]\n  result[site=B,run=1]\n  policy[site=A]\n  policy[site=B]\n",
    )
    .unwrap();
    assert_eq!(dag.jobs.len(), 2);
    assert_eq!(dag.jobs[0].inputs[0].len(), 2);
    assert_eq!(dag.jobs[0].inputs[1][0].to_string(), "policy[site=A]");
    assert_eq!(outputs(&dag), ["summary[site=A]", "summary[site=B]"]);
}

#[test]
fn a_single_input_of_an_aggregate_must_match_each_group() {
    let missing = resolve_text(COMBINE, "sources:\n  result[site=A,run=1]\n");
    assert!(matches!(
        missing,
        Err(ResolveError::MissingInput { port, product, .. }) if port == "policy" && product == "policy"
    ));
    let text = COMBINE.replace("source policy [site]", "source policy [site, run]");
    let pipeline = parse_pipeline(&text).unwrap();
    assert!(matches!(
        spit::validate_pipeline(&pipeline),
        Err(ResolveError::UnsupportedShapeRelationship { detail, .. })
            if detail.contains("input `policy` has dimensions absent from the groups of `result` @ vary(run): run")
    ));
}

#[test]
fn an_operation_takes_at_most_one_many_input() {
    let error = parse_pipeline("operation pair(a: many A, b: many B) -> C\n").unwrap_err();
    assert!(
        error.message.contains("at most one `many` input"),
        "{error}"
    );
}

const CALIBRATE: &str = "\
source signal [site, run]
source calibration [site, revision]
operation apply(data: Signal, calibration: Calibration) -> Signal
";

#[test]
fn where_pins_a_dimension_the_driver_lacks() {
    let text = format!("{CALIBRATE}calibrated = apply(signal, calibration @ where(revision=2))\n");
    let dag = resolve_text(
        &text,
        "sources:\n  signal[site=A,run=1]\n  calibration[site=A,revision=1]\n  calibration[site=A,revision=2]\n",
    )
    .unwrap();
    assert_eq!(dag.jobs.len(), 1);
    assert_eq!(
        dag.jobs[0].inputs[1][0].to_string(),
        "calibration[revision=2,site=A]"
    );
    assert_eq!(outputs(&dag), ["calibrated[run=1,site=A]"]);
}

#[test]
fn where_filters_the_driver_and_removes_its_dimension_from_the_output() {
    let text = "source image [site, echo]\noperation keep(Image) -> Image\nfirst = keep(image @ where(echo=1))\n";
    let (pipeline, _) = parse_document(text).unwrap();
    assert_eq!(pipeline.products[1].dimensions, ["site"]);
    let dag = resolve(
        &pipeline,
        &parse_source_inventory("sources:\n  image[site=A,echo=1]\n  image[site=A,echo=2]\n")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(outputs(&dag), ["first[site=A]"]);
}

#[test]
fn selectors_combine_on_one_binding() {
    let text = "source frame [subject, acq, run]\noperation stack(frames: many Frame) -> Stack @ drop(run)\nstacked = stack(frame @ where(acq=fast) @ vary(run))\n";
    let dag = resolve_text(
        text,
        "sources:\n  frame[subject=a,acq=fast,run=1]\n  frame[subject=a,acq=fast,run=2]\n  frame[subject=a,acq=slow,run=1]\n",
    )
    .unwrap();
    assert_eq!(outputs(&dag), ["stacked[subject=a]"]);
    assert_eq!(dag.jobs[0].inputs[0].len(), 2);
}

#[test]
fn same_matches_on_fewer_dimensions_and_requires_one_artifact() {
    let text = format!("{CALIBRATE}calibrated = apply(signal, calibration @ same(site))\n");
    let dag = resolve_text(
        &text,
        "sources:\n  signal[site=A,run=1]\n  calibration[site=A,revision=1]\n",
    )
    .unwrap();
    assert_eq!(dag.jobs.len(), 1);
    let ambiguous = resolve_text(
        &text,
        "sources:\n  signal[site=A,run=1]\n  calibration[site=A,revision=1]\n  calibration[site=A,revision=2]\n",
    );
    assert!(matches!(
        ambiguous,
        Err(ResolveError::AmbiguousInput { port, .. }) if port == "calibration"
    ));
}

#[test]
fn selectors_are_checked_against_the_port_and_product() {
    for (call, expected) in [
        (
            "apply(signal, calibration @ where(visit=1))",
            "has no dimension `visit`",
        ),
        (
            "apply(signal, calibration @ same(run))",
            "has no unpinned dimension `run`",
        ),
        (
            "apply(signal @ vary(run), calibration @ where(revision=1))",
            "`@ vary(...)` applies to many inputs",
        ),
    ] {
        let text = format!("{CALIBRATE}calibrated = {call}\n");
        let error = spit::validate_pipeline(&parse_pipeline(&text).unwrap()).unwrap_err();
        assert!(error.to_string().contains(expected), "{call}: {error}");
    }
    let error = parse_pipeline(&format!(
        "{CALIBRATE}x = apply(signal @ pick(run), calibration)\n"
    ))
    .unwrap_err();
    assert!(
        error.message.contains("`@ where(dimension=value, ...)`"),
        "{error}"
    );
}

const PREDICT: &str = "\
source reading [station]
source model [scenario]
source parameters [scenario]
operation predict(reading: Series, model: Model, parameters: Parameters) -> Matrix
";

const SCENARIOS: &str = "sources:\n  reading[station=01]\n  reading[station=02]\n  model[scenario=base]\n  model[scenario=high]\n  parameters[scenario=base]\n  parameters[scenario=high]\n";

#[test]
fn each_runs_a_step_for_every_value_an_input_broadcasts() {
    let text =
        format!("{PREDICT}forecast = predict(reading, model @ each(scenario), parameters)\n");
    let (pipeline, _) = parse_document(&text).unwrap();
    assert_eq!(pipeline.products[3].dimensions, ["station", "scenario"]);
    let dag = resolve(&pipeline, &parse_source_inventory(SCENARIOS).unwrap()).unwrap();
    assert_eq!(
        outputs(&dag),
        [
            "forecast[scenario=base,station=01]",
            "forecast[scenario=high,station=01]",
            "forecast[scenario=base,station=02]",
            "forecast[scenario=high,station=02]",
        ]
    );
    // Another input is matched on the broadcast dimension.
    assert_eq!(
        dag.jobs[1].inputs[2][0].to_string(),
        "parameters[scenario=high]"
    );
    let missing = resolve_text(
        &text,
        &SCENARIOS.replace("  parameters[scenario=high]\n", ""),
    );
    assert!(matches!(
        missing,
        Err(ResolveError::MissingInput { port, context, .. })
            if port == "parameters" && context.to_string().contains("scenario=high")
    ));
}

#[test]
fn a_broadcast_dimension_can_be_collected_again() {
    let text = "\
source reading [station]
source seed [rep]
operation simulate(reading: Series, seed: Seed) -> Series
trial = simulate(reading, seed @ each(rep))
operation average(items: many Series) -> Series @ drop(rep)
summary = average(trial @ vary(rep))
";
    let dag = resolve_text(
        text,
        "sources:\n  reading[station=01]\n  seed[rep=1]\n  seed[rep=2]\n  seed[rep=10]\n",
    )
    .unwrap();
    assert_eq!(
        outputs(&dag),
        [
            "trial[rep=1,station=01]",
            "trial[rep=2,station=01]",
            "trial[rep=10,station=01]",
            "summary[station=01]",
        ]
    );
    assert_eq!(dag.jobs[3].inputs[0].len(), 3);
}

#[test]
fn broadcasts_are_checked_against_the_step() {
    for (call, expected) in [
        (
            "predict(reading, model @ each(station), parameters)",
            "has no unpinned dimension `station`",
        ),
        (
            "predict(reading, model @ where(scenario=base) @ each(scenario), parameters)",
            "has no unpinned dimension `scenario`",
        ),
        (
            "predict(reading, model @ each(scenario), parameters @ each(scenario))",
            "`model` and `parameters` both broadcast `scenario`",
        ),
        (
            "predict(reading @ each(station), model @ each(scenario), parameters @ same(scenario))",
            "none can drive the step",
        ),
    ] {
        let text = format!("{PREDICT}forecast = {call}\n");
        let error = spit::validate_pipeline(&parse_pipeline(&text).unwrap()).unwrap_err();
        assert!(error.to_string().contains(expected), "{call}: {error}");
    }
    let text = "\
source reading [station, scenario]
source model [scenario]
operation fit(reading: Series, model: Model) -> Series
fitted = fit(reading, model @ each(scenario))
operation stack(items: many Series, model: Model) -> Stack @ drop(scenario)
stacked = stack(fitted @ vary(scenario), model @ each(scenario))
";
    let error = spit::validate_pipeline(&parse_pipeline(text).unwrap()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("driving product `reading` already has `scenario`"),
        "{error}"
    );
    let text = text.replace(
        "fitted = fit(reading, model @ each(scenario))",
        "fitted = fit(reading, model)",
    );
    let error = spit::validate_pipeline(&parse_pipeline(&text).unwrap()).unwrap_err();
    assert!(
        error.to_string().contains("would restore the dimension"),
        "{error}"
    );
    let error = parse_pipeline(&format!(
        "{PREDICT}x = predict(reading, model @ each(scenario, scenario), parameters)\n"
    ))
    .unwrap_err();
    assert!(error.message.contains("names `scenario` twice"), "{error}");
}

#[test]
fn port_order_does_not_decide_the_driving_input() {
    let text = "\
source signal [site, run]
source calibration [site]
operation apply(calibration: Calibration, data: Signal) -> Signal
calibrated = apply(calibration, signal)
";
    let (pipeline, _) = parse_document(text).unwrap();
    assert_eq!(pipeline.products[2].dimensions, ["site", "run"]);
    let dag = resolve(
        &pipeline,
        &parse_source_inventory(
            "sources:\n  signal[site=A,run=1]\n  signal[site=A,run=2]\n  calibration[site=A]\n",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(dag.jobs.len(), 2);
}

#[test]
fn collections_and_jobs_follow_natural_declared_order() {
    let text = "\
source frame [subject, run]
operation stack(frames: many Frame) -> Stack @ drop(run)
command stack: stack {frames} {output}
stacked = stack(frame @ vary(run))
path: {product}/{subject}/{run}.txt
path stacked: {product}/{subject}.txt
";
    let pipeline = parse_pipeline(text).unwrap();
    let inventory = parse_source_inventory(
        "sources:\n  frame[subject=s10,run=10]\n  frame[subject=s10,run=2]\n  frame[subject=s2,run=1]\n  frame[subject=s10,run=1]\n",
    )
    .unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(
        outputs(&dag),
        ["stacked[subject=s2]", "stacked[subject=s10]"]
    );
    let runs: Vec<_> = dag.jobs[1].inputs[0]
        .iter()
        .map(|frame| frame.entities.0["run"].clone())
        .collect();
    assert_eq!(runs, ["1", "2", "10"]);
    let script = render_bash(&pipeline, &dag).unwrap();
    assert!(script.contains(
        "'stack' \"$SPIT_ROOT\"/'frame/s10/1.txt' \"$SPIT_ROOT\"/'frame/s10/2.txt' \"$SPIT_ROOT\"/'frame/s10/10.txt'"
    ));
}

#[test]
fn min_rejects_a_collection_that_is_too_small() {
    let text = "source frame [subject, run]\noperation stack(frames: many Frame) -> Stack @ drop(run) @ min(2)\nstacked = stack(frame @ vary(run))\n";
    let result = resolve_text(
        text,
        "sources:\n  frame[subject=a,run=1]\n  frame[subject=a,run=2]\n  frame[subject=b,run=1]\n",
    );
    assert!(matches!(
        result,
        Err(ResolveError::CollectionTooSmall { minimum: 2, found: 1, context, .. })
            if context.to_string() == "subject=b"
    ));
    for (declaration, expected) in [
        (
            "operation f(A) -> B @ min(2)",
            "`@ min(count)` requires a many input",
        ),
        (
            "operation f(many A) -> B @ min(0)",
            "needs a positive integer",
        ),
    ] {
        let error = parse_pipeline(&format!("{declaration}\n")).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
    }
}

const TISSUES: &str = "\
path: {product}/{entities}.txt
source dwi : DWI [subject]
source mask : Mask [subject]
operation responses(dwi: DWI) -> (wm: Response, csf: Response)
command responses: estimate {dwi} {wm} {csf}
operation fods(dwi: DWI, wm_response: Response, csf_response: Response, mask: Mask) -> (wm: FOD, csf: FOD)
verify fods: same_grid {dwi} {mask}
command fods: fit {dwi} {wm_response} {wm} {csf_response} {csf} -mask {mask}
wm_response, csf_response = responses(dwi)
wm_fod, csf_fod = fods(dwi, wm_response, csf_response, mask)
";

#[test]
fn one_job_owns_every_output_of_an_operation() {
    let pipeline = parse_pipeline(TISSUES).unwrap();
    let inventory =
        parse_source_inventory("sources:\n  dwi[subject=a]\n  mask[subject=a]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 2);
    assert_eq!(
        outputs(&dag),
        [
            "wm_response[subject=a]",
            "csf_response[subject=a]",
            "wm_fod[subject=a]",
            "csf_fod[subject=a]"
        ]
    );
    assert_eq!(dag.jobs[1].dependencies, [1]);
    assert!(render_dag(&dag).contains("  outputs:\n    wm_fod[subject=a] : FOD\n"));
    assert!(render_bound_dag(&pipeline, &dag)
        .unwrap()
        .contains("    csf: csf_fod[subject=a] : FOD\n      path: csf_fod/subject=a.txt\n"));

    let script = render_bash(&pipeline, &dag).unwrap();
    assert_eq!(script.matches("'estimate'").count(), 1);
    assert!(script.contains("'estimate' \"$SPIT_ROOT\"/'dwi/subject=a.txt' \"$SPIT_ROOT\"/'wm_response/subject=a.txt' \"$SPIT_ROOT\"/'csf_response/subject=a.txt'"));
    assert!(script.contains("spit_require \"$SPIT_ROOT\"/'csf_response/subject=a.txt'"));
    // The verification runs before the job's command.
    let verify = script.find("spit_verify 2 'same_grid'").unwrap();
    assert!(verify < script.find("'fit'").unwrap());
}

#[test]
fn outputs_and_verifications_are_checked_before_resolution() {
    let wrong_count = TISSUES.replace("wm_fod, csf_fod = fods", "wm_fod = fods");
    let error = spit::validate_pipeline(&parse_pipeline(&wrong_count).unwrap()).unwrap_err();
    assert!(error
        .to_string()
        .contains("expected 2 output products (wm, csf), found 1"));

    let unwritten = TISSUES.replace(" {csf}\n", "\n");
    let error = validate_commands(&parse_pipeline(&unwritten).unwrap()).unwrap_err();
    assert!(error.message().contains("must use `{csf}`"), "{error}");

    let reads_output = TISSUES.replace("same_grid {dwi} {mask}", "same_grid {wm}");
    let error = validate_commands(&parse_pipeline(&reads_output).unwrap()).unwrap_err();
    assert!(
        error
            .message()
            .contains("verify for `fods` cannot use output `{wm}`"),
        "{error}"
    );
}

#[test]
fn coverage_rules_can_require_entity_values() {
    let text = "source image [subject, run]\nrequire image run=1,2 per [subject]\noperation f(Image) -> Image\nout = f(image)\n";
    let complete = "sources:\n  image[subject=a,run=1]\n  image[subject=a,run=2]\n";
    assert!(resolve_text(text, complete).is_ok());
    let incomplete = "sources:\n  image[subject=a,run=1]\n  image[subject=a,run=3]\n";
    assert!(matches!(
        resolve_text(text, incomplete),
        Err(ResolveError::MissingRequiredValue { dimension, value, .. })
            if dimension == "run" && value == "2"
    ));
    let issues: Vec<_> = diagnose(text, Some(incomplete))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        issues,
        ["error: line 2: source coverage for `image` at [subject=a]: no artifact with run=2"]
    );
    let grouped = text.replace("run=1,2 per [subject]", "subject=a per [subject]");
    let error = spit::validate_pipeline(&parse_pipeline(&grouped).unwrap()).unwrap_err();
    assert!(error.to_string().contains("outside its groups"), "{error}");
}

#[test]
fn steps_that_resolve_no_jobs_are_reported() {
    let text = "\
source image [subject]
source extra [subject]
operation f(Image) -> Image
operation g(Image, Image) -> Image
cleaned = f(image)
other = f(extra @ where(subject=z))
both = g(cleaned, image)
";
    assert_eq!(
        warnings(diagnose(text, Some("sources:\n  extra[subject=a]\n"))),
        [
            "warning: line 1: source `image` has no artifacts in the inventory, so these steps resolve no jobs: cleaned, both",
            "warning: line 6: `other` resolves no jobs: its inputs have artifacts, but none match each other or the step's selectors",
        ]
    );
    // Without an inventory, no step is expected to resolve jobs.
    assert!(warnings(diagnose(text, None)).is_empty());
}

#[test]
fn a_separate_inventory_replaces_a_malformed_inline_one() {
    let text = "source image [subject]\noperation f(Image) -> Image\nout = f(image)\nsources:\n  image[subject=a\n";
    assert!(diagnose(text, None).iter().any(Diagnostic::is_error));
    let issues: Vec<_> = diagnose(text, Some("sources:\n  image[subject=b]\n"))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        issues,
        ["warning: line 4: this inline inventory is ignored because a separate inventory was supplied"]
    );
}

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str, files: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!("spit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        Self(root)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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
path: results/{product}/{entities}.txt
source image [sub, ses]
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
operation process(image) -> Output
output = process(image)
";
    let pipeline = parse_pipeline(text).unwrap();
    let inventory = discover_sources(&pipeline, &tree.0).unwrap();
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

    let pipeline_file = tree.0.join("pipeline.spit");
    fs::write(&pipeline_file, text).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "discover",
            pipeline_file.to_str().unwrap(),
            "--root",
            tree.0.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = String::from_utf8(output.stdout).unwrap();
    assert!(
        rendered.contains("contexts sessions:\n    [ses=baseline,sub=A]"),
        "{rendered}"
    );
    assert!(rendered.contains("image[sub=A,ses=baseline]"), "{rendered}");
    let implicit = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["discover", pipeline_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        implicit.status.success(),
        "{}",
        String::from_utf8_lossy(&implicit.stderr)
    );
    assert_eq!(implicit.stdout, rendered.as_bytes());
    let dag = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            pipeline_file.to_str().unwrap(),
            "--root",
            tree.0.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        dag.status.success(),
        "{}",
        String::from_utf8_lossy(&dag.stderr)
    );
    let json = String::from_utf8(dag.stdout).unwrap();
    assert!(json.contains("\"operation\":\"process\""), "{json}");
    assert!(json.contains("\"sub\":\"A\""), "{json}");
    let checked = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", pipeline_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(String::from_utf8(checked.stdout)
        .unwrap()
        .contains("3 jobs resolved"));
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
    let pipeline = parse_pipeline(text).unwrap();
    let error = discover_sources(&pipeline, &tree.0).unwrap_err();
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
    let pipeline = parse_pipeline(
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         source image [sub, ses]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n\
         source reference [sub]\n\
         path reference: data/sub-{sub}/reference.nii.gz\n",
    )
    .unwrap();
    let inventory = discover_sources(&pipeline, &tree.0).unwrap();
    let references: Vec<_> = inventory
        .artifacts
        .iter()
        .filter(|record| record.product == "reference")
        .map(|record| record.entities.0["sub"].as_str())
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
    let pipeline = parse_pipeline(text).unwrap();
    let inventory = discover_sources(&pipeline, &tree.0).unwrap();
    assert_eq!(inventory.discovered["sessions"].len(), 9);
    let rendered = spit::render_source_inventory(&inventory, &pipeline);
    assert!(rendered.starts_with("contexts sessions:\n"), "{rendered}");
    assert_eq!(parse_source_inventory(&rendered).unwrap(), inventory);
    let inline = format!("{text}{rendered}");
    let (inline_pipeline, Some(inline_inventory)) = parse_document(&inline).unwrap() else {
        panic!("expected inline named contexts");
    };
    assert!(matches!(
        resolve(&inline_pipeline, &inline_inventory),
        Err(ResolveError::CoverageViolation {
            found: 1,
            discovery: true,
            ..
        })
    ));
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::CoverageViolation { product, context, found: 1, discovery: true, .. })
            if product == "sessions" && context.0.get("sub").map(String::as_str) == Some("5")
    ));
    let pipeline_file = tree.0.join("pipeline.spit");
    fs::write(&pipeline_file, text).unwrap();
    let discovered = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["discover", pipeline_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        discovered.status.success(),
        "{}",
        String::from_utf8_lossy(&discovered.stderr)
    );
    assert_eq!(String::from_utf8(discovered.stdout).unwrap(), rendered);
    let checked = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", pipeline_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!checked.status.success());
    let errors = String::from_utf8(checked.stderr).unwrap();
    assert!(errors.contains("line 2"), "{errors}");
    assert!(
        errors.contains(
            "discovery coverage for `sessions` at [sub=5]: expected at least 2 binding(s), found 1"
        ),
        "{errors}"
    );

    let values = text.replace("count>=2", "ses=1,2");
    let pipeline = parse_pipeline(&values).unwrap();
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::MissingRequiredValue { product, context, dimension, value, discovery: true, .. })
            if product == "sessions" && context.0.get("sub").map(String::as_str) == Some("5") && dimension == "ses" && value == "2"
    ));
}

#[test]
fn skip_discovery_group_removes_subject_before_source_checks_and_jobs() {
    let tree = Tree::new(
        "directory-skip",
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
                skip sessions count>=2 per [sub]\n\
                require sessions count>=2 per [sub]\n\
                source image [sub, ses]\n\
                path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n\
                operation process(Image) -> Image\n\
                result = process(image)\n\
                path result: out/sub-{sub}/ses-{ses}/result.nii.gz\n";
    let pipeline = parse_pipeline(text).unwrap();
    let inventory = discover_sources(&pipeline, &tree.0).unwrap();
    assert_eq!(inventory.discovered["sessions"].len(), 8);
    assert_eq!(inventory.artifacts.len(), 8);
    assert!(!inventory
        .contexts
        .iter()
        .any(|binding| binding.0["sub"] == "5"));
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
    let dag = resolve(&pipeline, &explicit).unwrap();
    assert_eq!(dag.jobs.len(), 8);
    assert!(!outputs(&dag).iter().any(|output| output.contains("sub=5")));

    let pipeline_file = tree.0.join("pipeline.spit");
    fs::write(&pipeline_file, text).unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", pipeline_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(String::from_utf8_lossy(&checked.stderr).contains("skip sessions"));
    let dag = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["dag", pipeline_file.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        dag.status.success(),
        "{}",
        String::from_utf8_lossy(&dag.stderr)
    );
    assert!(!String::from_utf8_lossy(&dag.stdout).contains("sub=5"));
}

#[test]
fn skip_source_group_can_omit_missing_files_in_a_discovered_context() {
    let tree = Tree::new(
        "source-skip",
        &[
            "data/sub-1/ses-1/image.nii.gz",
            "data/sub-1/ses-2/image.nii.gz",
            "data/sub-5/ses-1/image.nii.gz",
            "data/sub-5/ses-2/.keep",
        ],
    );
    let pipeline = parse_pipeline(
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         source image [sub, ses]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n\
         skip image count>=2 per [sub]\n\
         operation process(Image) -> Image\n\
         result = process(image)\n",
    )
    .unwrap();
    let inventory = discover_sources(&pipeline, &tree.0).unwrap();
    assert_eq!(inventory.artifacts.len(), 2);
    assert!(inventory
        .contexts
        .iter()
        .all(|binding| binding.0["sub"] == "1"));
    assert_eq!(resolve(&pipeline, &inventory).unwrap().jobs.len(), 2);
}

#[test]
fn skip_does_not_hide_invalid_inventory_bindings() {
    let pipeline = parse_pipeline(
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         skip sessions count>=2 per [sub]\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("contexts sessions:\n[sub=5]\n").unwrap();
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::InvalidDefinition { detail, .. }) if detail.contains("must bind [sub, ses]")
    ));
}

#[test]
fn sibling_spitin_discovers_inputs_and_defaults_output_paths() {
    let tree = Tree::new(
        "spitin-sibling",
        &[
            "data/sub-1/ses-1/image.nii.gz",
            "data/sub-1/ses-2/image.nii.gz",
            "data/sub-5/ses-1/.keep",
        ],
    );
    let pipeline_file = tree.0.join("analysis.spit");
    fs::write(
        &pipeline_file,
        "source image: Image [sub, ses]\n\
         operation process(Image) -> Image\n\
         result = process(image)\n",
    )
    .unwrap();
    fs::write(
        tree.0.join("analysis.spitin"),
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         skip sessions count>=2 per [sub]\n\
         require sessions count>=2 per [sub]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n",
    )
    .unwrap();
    let check = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", pipeline_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stderr).contains("skip sessions"));
    assert!(String::from_utf8_lossy(&check.stdout).contains("2 jobs resolved"));
    let paths = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["dag", pipeline_file.to_str().unwrap(), "--paths"])
        .output()
        .unwrap();
    assert!(
        paths.status.success(),
        "{}",
        String::from_utf8_lossy(&paths.stderr)
    );
    let paths = String::from_utf8(paths.stdout).unwrap();
    assert!(paths.contains("out/result/sub=1__ses=1"), "{paths}");
    assert!(!paths.contains("sub=5"), "{paths}");
}

#[test]
fn explicit_spitin_uses_its_own_directory_and_require_reports_gaps() {
    let tree = Tree::new(
        "spitin-explicit",
        &[
            "dataset/data/sub-1/ses-1/image.nii.gz",
            "dataset/data/sub-1/ses-2/image.nii.gz",
            "dataset/data/sub-5/ses-1/image.nii.gz",
        ],
    );
    let pipeline_file = tree.0.join("pipeline.spit");
    fs::write(
        &pipeline_file,
        "source image [sub, ses]\n\
         operation process(Image) -> Image\n\
         result = process(image)\n",
    )
    .unwrap();
    let recipe = tree.0.join("dataset/inputs.spitin");
    fs::write(
        &recipe,
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         require sessions count>=2 per [sub]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n",
    )
    .unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            pipeline_file.to_str().unwrap(),
            "--inputs",
            recipe.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stderr)
        .contains("discovery coverage for `sessions` at [sub=5]"));
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
    let pipeline = parse_pipeline(
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         discover controls: [sub, ses] from dirs controls/sub-{sub}/ses-{ses}\n\
         require sessions count>=2 per [sub]\n",
    )
    .unwrap();
    let inventory = discover_sources(&pipeline, &tree.0).unwrap();
    assert_eq!(inventory.contexts.len(), 2);
    assert_eq!(inventory.discovered["sessions"].len(), 1);
    assert_eq!(inventory.discovered["controls"].len(), 2);
    assert!(matches!(
        resolve(&pipeline, &inventory),
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
    let pipeline =
        parse_pipeline("discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n")
            .unwrap();
    let error = discover_sources(&pipeline, &tree.0).unwrap_err();
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
    let pipeline_file = tree.0.join("pipeline.spit");
    fs::write(
        &pipeline_file,
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n",
    )
    .unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            pipeline_file.to_str().unwrap(),
            "--root",
            tree.0.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(checked.status.success());
    let diagnostics = String::from_utf8(checked.stdout).unwrap();
    assert!(
        diagnostics.contains("matched no directories"),
        "{diagnostics}"
    );
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
fn cli_skips_an_inline_inventory_that_sources_replaces() {
    let tree = Tree::new("override", &[]);
    fs::create_dir_all(&tree.0).unwrap();
    let pipeline = tree.0.join("pipeline.spit");
    fs::write(
        &pipeline,
        "source image [subject]\noperation f(Image) -> Image\nout = f(image)\nsources:\n  image[subject=a\n",
    )
    .unwrap();
    let inventory = tree.0.join("inventory.sources");
    fs::write(&inventory, "sources:\n  image[subject=b]\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            pipeline.to_str().unwrap(),
            "--sources",
            inventory.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert!(String::from_utf8(result.stdout)
        .unwrap()
        .contains("1 jobs resolved."));
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("this inline inventory is ignored"));
}
