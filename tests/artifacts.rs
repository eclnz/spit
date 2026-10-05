mod support;

use std::process::Command;

use spit::{
    diagnose, parse_source_inventory, render_artifacts, render_artifacts_by_target, resolve,
    resolve_artifacts_excluding, resolve_artifacts_partial, ArtifactReport, Gap, InputSource,
    ResolveError, ResolvedInputs, Severity,
};
use support::{numbers, Tree};

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

/// The report of `text` over `inventory`, as `spit artifacts` prints it.
fn rendered(text: &str, inventory: &str, report: &ArtifactReport) -> String {
    let (pipeline, _) = settle(text, inventory).unwrap();
    render_artifacts(&pipeline, report)
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
        .map(|&output| report.dag.artifact(output).to_string())
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
operation clean(scan: Scan) -> Scan
cleaned = clean(scan)
operation align(moving: Scan, reference: Calibration) -> Scan
aligned = align(cleaned, calibration)
operation merge(runs: many Scan) -> Scan
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
    let ids: Vec<_> = report.dag.jobs.iter().map(|job| job.id.number()).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5, 6]);
    assert_eq!(numbers(&report.dag.jobs[5].dependencies), [4, 5]);

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
fn partial_keeps_an_aggregate_incomplete_when_no_member_is_complete() {
    let (pipeline, settled) = settle(ALIGN, ALIGN_SOURCES).unwrap();
    let partial = resolve_artifacts_partial(&pipeline, &settled.dag_inventory(), &[]).unwrap();
    assert!(incomplete(&partial)
        .iter()
        .any(|artifact| artifact == "merged[subject=02]"));
}

#[test]
fn a_numeric_near_miss_points_to_the_existing_artifact() {
    let sources = ALIGN_SOURCES.replace("calibration[subject=01]", "calibration[subject=001]");
    let report = report(ALIGN, &sources).unwrap();
    let rendered = rendered(ALIGN, &sources, &report);
    assert!(
        rendered.contains(
            "calibration[subject=001] exists; its `subject` differs only in leading zeros"
        ),
        "{rendered}"
    );
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
operation summarise(days: many Day @ min(2)) -> Summary
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
require [subject] where scan has run=1,2,3
operation clean(scan: Scan) -> Scan
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
        [Gap::Blocked { port, .. }] if port == "scan"
    ));
    assert_eq!(first_error(text, sources), report.coverage[0].error);

    let rendered = rendered(text, sources, &report);
    assert!(rendered.contains("  scan[subject=01,run=1]  (source)\n"));
    assert!(!rendered.contains("  scan[subject=02,run=1]  (source)\n"));
    assert!(rendered.contains(
        "    - input `scan` needs scan[subject=02,run=1], which a coverage gap holds back\n"
    ));
    assert!(rendered.contains("Coverage gaps: 2\n"));
    assert!(rendered.contains("    holds back: scan[subject=02,run=1]\n"));
    // A source a gap holds back is reported with the gap, not as unused.
    assert!(report.unused_sources().is_empty());
    assert!(!rendered.contains("Unused sources"));
}

#[test]
fn a_source_no_job_reads_is_listed_as_unused() {
    // Revision 1 is filtered out by `where`, and `price[store=S07]` names a
    // store no sales need. Store s09's sales have no price list, so their
    // job cannot be completed, but that job still reads them.
    let text = "\
source cal [station, revision]
source reading [station]
source sales [store]
source price [store]
operation calibrate(cal, reading) -> Reading
calibrated = calibrate(cal @ where(revision=2), reading)
operation total(sales, price) -> Total
totals = total(sales, price)
";
    let sources = "\
sources:
    cal[station=north,revision=1]
    cal[station=north,revision=2]
    reading[station=north]
    sales[store=s07]
    sales[store=s09]
    price[store=S07]
    price[store=s07]
";
    let report = report(text, sources).unwrap();
    let unused: Vec<_> = report
        .unused_sources()
        .into_iter()
        .map(|id| report.dag.artifact(id).to_string())
        .collect();
    assert_eq!(
        unused,
        ["cal[revision=1,station=north]", "price[store=S07]"]
    );
    assert_eq!(incomplete(&report), ["totals[store=s09]"]);
    assert_eq!(
        spit::unused_sources_summary(&report).unwrap(),
        "2 source artifacts are used by no job: cal[station=north,revision=1], price[store=S07]"
    );
    let rendered = rendered(text, sources, &report);
    assert!(
        rendered
            .contains("\nUnused sources: 2\n  cal[station=north,revision=1]\n  price[store=S07]\n"),
        "{rendered}"
    );
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
    let rendered = rendered(ALIGN, ALIGN_SOURCES, &report(ALIGN, ALIGN_SOURCES).unwrap());
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

#[test]
fn by_target_nests_what_each_final_target_waits_on() {
    let (pipeline, _) = settle(ALIGN, ALIGN_SOURCES).unwrap();
    let report = report(ALIGN, ALIGN_SOURCES).unwrap();
    assert_eq!(
        render_artifacts_by_target(&pipeline, &report),
        "\
Complete artifacts: 10

Final targets that cannot be made: 1 (incomplete artifacts: 2)
  merged[subject=02] : Scan  (merge)
    aligned[subject=02,run=1] : Scan  (align)
      - no `calibration` artifact for input `reference` of `align` at [run=1,subject=02]
"
    );
}

#[test]
fn by_target_gives_each_target_its_own_tree_and_shows_an_artifact_once_under_it() {
    let text = "\
source day [site, date]
source rate [site]
operation price(day: Day, rate: Rate) -> Priced
operation total(days: many Priced) -> Total
operation both(total: Total) -> Both
priced = price(day, rate)
total = total(priced @ vary(date))
both = both(total)
";
    let sources = "\
sources:
    day[site=a,date=1]
    day[site=a,date=2]
    day[site=b,date=1]
    rate[site=a]
";
    let (pipeline, _) = settle(text, sources).unwrap();
    let report = report(text, sources).unwrap();
    let grouped = render_artifacts_by_target(&pipeline, &report);
    // `both[site=b]` is the only final target of site b; site a is complete.
    assert!(
        grouped.contains("Final targets that cannot be made: 1 (incomplete artifacts: 3)\n"),
        "{grouped}"
    );
    assert_eq!(
        grouped.matches("priced[site=b,date=1]").count(),
        1,
        "{grouped}"
    );
    assert_eq!(grouped.matches("total[site=b]").count(), 1, "{grouped}");
    // The order is the same every time.
    assert_eq!(grouped, render_artifacts_by_target(&pipeline, &report));
}

#[test]
fn by_target_on_a_complete_dataset_has_no_targets_section() {
    let text = "\
source day [site]
operation summarise(day: Day) -> Summary
summary = summarise(day)
";
    let sources = "sources:\n    day[site=a]\n";
    let (pipeline, _) = settle(text, sources).unwrap();
    let report = report(text, sources).unwrap();
    let grouped = render_artifacts_by_target(&pipeline, &report);
    assert!(grouped.starts_with("Complete artifacts: 2\n"), "{grouped}");
    assert!(
        grouped.contains("Final targets that cannot be made: 0"),
        "{grouped}"
    );
}

/// A step joining two branches that both read a product whose second input
/// is missing.
const DIAMOND: &str = "\
source s [k]
source t [k]
operation mk(s: S, t: T) -> A
operation left(a: A) -> B
operation right(a: A) -> C
operation join(b: B, c: C) -> D
a = mk(s, t)
bb = left(a)
cc = right(a)
dd = join(bb, cc)
";

#[test]
fn by_target_names_an_artifact_two_branches_wait_on_where_it_is_shown() {
    let sources = "sources:\n    s[k=1]\n";
    let (pipeline, _) = settle(DIAMOND, sources).unwrap();
    let report = report(DIAMOND, sources).unwrap();
    assert_eq!(
        render_artifacts_by_target(&pipeline, &report),
        "\
Complete artifacts: 1

Final targets that cannot be made: 1 (incomplete artifacts: 4)
  dd[k=1]  (join)
    bb[k=1]  (left)
      a[k=1]  (mk)
        - no `t` artifact for input `t` of `mk` at [k=1]
    cc[k=1]  (right)
      - input `a` needs a[k=1], shown above
"
    );
}

#[test]
fn by_target_names_an_artifact_a_job_and_the_job_it_waits_on_both_need() {
    let text = "\
source s [k]
source t [k]
operation mk(s: S, t: T) -> A
operation fold(a: A, again: A) -> B
operation join(b: B, c: A) -> D
c = mk(s, t)
b = fold(c, c)
d = join(b, c)
";
    let sources = "sources:\n    s[k=1]\n";
    let (pipeline, _) = settle(text, sources).unwrap();
    let report = report(text, sources).unwrap();
    assert_eq!(
        render_artifacts_by_target(&pipeline, &report),
        "\
Complete artifacts: 1

Final targets that cannot be made: 1 (incomplete artifacts: 3)
  d[k=1]  (join)
    b[k=1]  (fold)
      c[k=1]  (mk)
        - no `t` artifact for input `t` of `mk` at [k=1]
      - input `again` needs c[k=1], shown above
    - input `c` needs c[k=1], shown above
"
    );
}

#[test]
fn by_target_writes_an_upstream_gap_once_for_all_the_targets_that_share_it() {
    let text = "\
source s [k]
source t
source g
operation make(t: T, g: G) -> Shared
operation use(s: S, shared: Shared) -> U
gg = make(t, g)
uu = use(s, gg)
";
    let sources = "sources:\n    s[k=1]\n    s[k=2]\n    s[k=3]\n    t\n";
    let (pipeline, _) = settle(text, sources).unwrap();
    let report = report(text, sources).unwrap();
    assert_eq!(
        render_artifacts_by_target(&pipeline, &report),
        "\
Complete artifacts: 4

Final targets that cannot be made: 3 (incomplete artifacts: 4)
  uu[k=1]  (use)
    gg : Shared  (make)
      - no `g` artifact for input `g` of `make` at []
  uu[k=2]  (use)
    - input `shared` needs gg, shown under uu[k=1]
  uu[k=3]  (use)
    - input `shared` needs gg, shown under uu[k=1]
"
    );
}

/// A chain of `steps` steps whose first reads a source that is missing, so
/// that every step is incomplete and the last is the only final target.
fn broken_chain(steps: usize) -> (String, &'static str) {
    let mut text = String::from(
        "source p0 [sub]\nsource miss [sub]\noperation first(a: T, b: T) -> T\noperation step(input: T) -> T\np1 = first(p0, miss)\n",
    );
    for index in 2..=steps {
        text += &format!("p{index} = step(p{})\n", index - 1);
    }
    (text, "sources:\n    p0[sub=1]\n")
}

#[test]
fn by_target_stops_indenting_past_twenty_levels() {
    let (text, sources) = broken_chain(24);
    let (pipeline, _) = settle(&text, sources).unwrap();
    let report = report(&text, sources).unwrap();
    let grouped = render_artifacts_by_target(&pipeline, &report);
    let indents: Vec<usize> = grouped
        .lines()
        .skip(3)
        .map(|line| line.len() - line.trim_start().len())
        .collect();
    // p24 is the target at the first indent; each step waits on the one
    // before, with the reason of p1 under it.
    assert_eq!(indents.len(), 25, "{grouped}");
    assert_eq!(indents[0], 2);
    assert_eq!(indents[19], 40);
    assert!(
        indents[20..].iter().all(|&indent| indent == 40),
        "{indents:?}"
    );
}

#[test]
fn by_target_output_grows_with_the_length_of_a_chain_not_its_square() {
    // The indent used to grow with the depth, so chains of 2,000 and 4,000
    // steps wrote about 4 MB and 16 MB: four times as much for twice the steps.
    let sizes: Vec<usize> = [2_000, 4_000]
        .into_iter()
        .map(|steps| {
            let (text, sources) = broken_chain(steps);
            let (pipeline, _) = settle(&text, sources).unwrap();
            let report = report(&text, sources).unwrap();
            let grouped = render_artifacts_by_target(&pipeline, &report);
            assert!(grouped.len() < 120 * steps, "{} bytes", grouped.len());
            grouped.len()
        })
        .collect();
    assert!(sizes[1] < 3 * sizes[0], "{sizes:?}");
}
