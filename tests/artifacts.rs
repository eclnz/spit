mod support;

use std::process::Command;

use spit::{
    diagnose, parse_source_inventory, render_artifacts, resolve, resolve_artifacts_excluding,
    ArtifactReport, Gap, InputSource, ResolveError, ResolvedInputs, Severity,
};
use support::Tree;

/// The document's pipeline, and `inventory` after the input stage.
fn settle(text: &str, inventory: &str) -> Result<(spit::Pipeline, ResolvedInputs), ResolveError> {
    let (pipeline, spec, _) = support::parse_with_rules(text).unwrap();
    let records = parse_source_inventory(inventory).unwrap();
    let settled = spec
        .resolve(&pipeline, InputSource::Inventory(records))
        .map_err(|error| match error {
            spit::InputError::Resolve(error) => error,
            error => panic!("{error}"),
        })?;
    Ok((pipeline, settled))
}

/// What `spit artifacts` reports: jobs over the settled inventory, with the
/// sources a coverage gap holds back.
fn report(text: &str, inventory: &str) -> Result<ArtifactReport, ResolveError> {
    let (pipeline, settled) = settle(text, inventory)?;
    let mut report =
        resolve_artifacts_excluding(&pipeline, &settled.dag_inventory(), &settled.unavailable())?;
    report.coverage = settled.gaps;
    Ok(report)
}

/// The error `spit dag` stops at: a missing requirement, else a job.
fn first_error(text: &str, inventory: &str) -> ResolveError {
    let (pipeline, settled) = match settle(text, inventory) {
        Ok(settled) => settled,
        Err(error) => return error,
    };
    if let Err(error) = settled.require_complete() {
        return error;
    }
    resolve(&pipeline, &settled.dag_inventory()).unwrap_err()
}

fn complete(report: &ArtifactReport) -> Vec<String> {
    report
        .dag
        .jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .map(ToString::to_string)
        .collect()
}

fn incomplete(report: &ArtifactReport) -> Vec<String> {
    report
        .incomplete
        .iter()
        .flat_map(|job| &job.outputs)
        .map(ToString::to_string)
        .collect()
}

const ALIGN: &str = "\
source scan [subject, run]
source calibration [subject]
operation clean(Scan) -> Scan
cleaned = clean(scan)
operation align(moving: Scan, reference: Calibration) -> Scan
aligned = align(cleaned, calibration)
operation merge(runs: many Scan) -> Scan @ drop(run)
merged = merge(aligned @ vary(run))
";

const ALIGN_SOURCES: &str = "\
sources:
    scan[subject=01,run=1]
    scan[subject=01,run=2]
    scan[subject=02,run=1]
    calibration[subject=01]
";

#[test]
fn keeps_complete_jobs_and_blocks_consumers_of_incomplete_ones() {
    let report = report(ALIGN, ALIGN_SOURCES).unwrap();

    assert_eq!(
        complete(&report),
        [
            "cleaned[run=1,subject=01]",
            "cleaned[run=2,subject=01]",
            "cleaned[run=1,subject=02]",
            "aligned[run=1,subject=01]",
            "aligned[run=2,subject=01]",
            "merged[subject=01]",
        ]
    );
    let ids: Vec<_> = report.dag.jobs.iter().map(|job| job.id).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5, 6]);
    assert_eq!(report.dag.jobs[5].dependencies, [4, 5]);

    assert_eq!(
        incomplete(&report),
        ["aligned[run=1,subject=02]", "merged[subject=02]"]
    );
    assert!(matches!(
        report.incomplete[0].gaps.as_slice(),
        [Gap::Unmatched(ResolveError::MissingInput { site: spit::PortSite { port, product, .. }, .. })]
            if port == "reference" && product == "calibration"
    ));
    assert!(matches!(
        report.incomplete[1].gaps.as_slice(),
        [Gap::Blocked { port, artifact }]
            if port == "runs" && artifact.to_string() == "aligned[run=1,subject=02]"
    ));
    assert_eq!(report.sources.len(), 4);
    assert!(report.coverage.is_empty());
}

#[test]
fn resolve_fails_with_the_first_gap_the_report_finds() {
    let report = report(ALIGN, ALIGN_SOURCES).unwrap();
    let Gap::Unmatched(expected) = &report.incomplete[0].gaps[0] else {
        panic!("the first incomplete job is not blocked");
    };
    assert_eq!(&first_error(ALIGN, ALIGN_SOURCES), expected);
}

#[test]
fn a_job_lists_every_gap_it_has() {
    let text = "\
source scan [subject, run]
source calibration [subject, site]
source mask [subject]
operation align(moving: Scan, reference: Calibration, mask: Mask) -> Scan
aligned = align(scan, calibration @ same(subject), mask)
";
    let sources = "\
sources:
    scan[subject=01,run=1]
    calibration[subject=01,site=a]
    calibration[subject=01,site=b]
";
    let report = report(text, sources).unwrap();
    assert!(report.dag.jobs.is_empty());
    assert!(matches!(
        report.incomplete[0].gaps.as_slice(),
        [
            Gap::Unmatched(ResolveError::AmbiguousInput { site: spit::PortSite { port: ambiguous, .. }, .. }),
            Gap::Unmatched(ResolveError::MissingInput { site: spit::PortSite { port: missing, .. }, .. }),
        ] if ambiguous == "reference" && missing == "mask"
    ));
}

#[test]
fn a_small_collection_leaves_other_groups_complete() {
    let text = "\
source day [site, date]
operation summarise(days: many Day) -> Summary @ drop(date) @ min(2)
summary = summarise(day @ vary(date))
";
    let sources = "\
sources:
    day[site=a,date=1]
    day[site=a,date=2]
    day[site=b,date=1]
";
    let report = report(text, sources).unwrap();
    assert_eq!(complete(&report), ["summary[site=a]"]);
    assert_eq!(incomplete(&report), ["summary[site=b]"]);
    assert!(matches!(
        report.incomplete[0].gaps.as_slice(),
        [Gap::Unmatched(ResolveError::CollectionTooSmall {
            minimum: 2,
            found: 1,
            ..
        })]
    ));
}

#[test]
fn a_coverage_gap_holds_back_its_sources_and_blocks_their_consumers() {
    let text = "\
source scan [subject, run]
require scan run=1,2,3 per [subject]
operation clean(Scan) -> Scan
cleaned = clean(scan)
";
    let sources = "\
sources:
    scan[subject=01,run=1]
    scan[subject=01,run=2]
    scan[subject=01,run=3]
    scan[subject=02,run=1]
";
    let report = report(text, sources).unwrap();

    let missing: Vec<_> = report
        .coverage
        .iter()
        .map(|gap| match &gap.error {
            ResolveError::MissingRequiredValue { value, .. } => value.as_str(),
            other => panic!("unexpected coverage error {other}"),
        })
        .collect();
    assert_eq!(missing, ["2", "3"]);
    let held: Vec<_> = report.coverage[0]
        .sources
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(held, ["scan[run=1,subject=02]"]);

    assert_eq!(complete(&report).len(), 3);
    assert_eq!(incomplete(&report), ["cleaned[run=1,subject=02]"]);
    assert!(matches!(
        report.incomplete[0].gaps.as_slice(),
        [Gap::Blocked { port, .. }] if port == "input"
    ));
    assert_eq!(first_error(text, sources), report.coverage[0].error);

    let rendered = render_artifacts(&report);
    assert!(rendered.contains("  scan[subject=01,run=1]  (source)\n"));
    assert!(!rendered.contains("  scan[subject=02,run=1]  (source)\n"));
    assert!(rendered.contains(
        "    - input `input` needs scan[subject=02,run=1], which a coverage gap holds back\n"
    ));
    assert!(rendered.contains("Coverage gaps: 2\n"));
    assert!(rendered.contains("    holds back: scan[subject=02,run=1]\n"));
}

#[test]
fn an_invalid_inventory_is_still_an_error() {
    let sources = "\
sources:
    scan[subject=01,run=1]
    scan[subject=01,run=1]
";
    assert!(matches!(
        report(ALIGN, sources),
        Err(ResolveError::DuplicateSourceArtifact { .. })
    ));
}

#[test]
fn render_lists_complete_then_incomplete_artifacts() {
    let rendered = render_artifacts(&report(ALIGN, ALIGN_SOURCES).unwrap());
    assert_eq!(
        rendered,
        "\
Complete artifacts: 10
  scan[subject=01,run=1]  (source)
  scan[subject=01,run=2]  (source)
  scan[subject=02,run=1]  (source)
  calibration[subject=01]  (source)
  cleaned[subject=01,run=1] : Scan  (job 1: clean)
  cleaned[subject=01,run=2] : Scan  (job 2: clean)
  cleaned[subject=02,run=1] : Scan  (job 3: clean)
  aligned[subject=01,run=1] : Scan  (job 4: align)
  aligned[subject=01,run=2] : Scan  (job 5: align)
  merged[subject=01] : Scan  (job 6: merge)

Incomplete artifacts: 2
  aligned[subject=02,run=1] : Scan  (align)
    - no `calibration` artifact for input `reference` of `align` at [run=1,subject=02]
  merged[subject=02] : Scan  (merge)
    - input `runs` needs aligned[subject=02,run=1], which cannot be produced
"
    );
}

#[test]
fn diagnose_still_reports_the_gap_as_an_error() {
    let errors: Vec<_> = diagnose(ALIGN, Some(ALIGN_SOURCES))
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1);
}

#[test]
fn cli_lists_incomplete_artifacts_where_check_fails() {
    let tree = Tree::new("cli-artifacts", &[]);
    let pipeline = tree.write("align.spit", ALIGN);
    let sources = tree.write("align.spitout", ALIGN_SOURCES);
    let run = |command: &str| {
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args([
                command,
                pipeline.to_str().unwrap(),
                sources.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let artifacts = run("artifacts");
    let check = run("dag");

    assert!(
        artifacts.status.success(),
        "{}",
        String::from_utf8_lossy(&artifacts.stderr)
    );
    assert!(artifacts.stderr.is_empty());
    let stdout = String::from_utf8(artifacts.stdout).unwrap();
    assert!(stdout.starts_with("Complete artifacts: 10\n"));
    assert!(stdout.contains("Incomplete artifacts: 2\n"));

    assert!(!check.status.success());
    assert!(String::from_utf8(check.stderr)
        .unwrap()
        .contains("no `calibration` artifact for input `reference`"));
}
