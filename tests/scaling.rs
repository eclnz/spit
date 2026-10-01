//! Each stage's time must grow about in step with the dataset. The test
//! times every stage on a small dataset and on one four times its size: work
//! in proportion to the dataset takes about four times as long, while work
//! that compares everything with everything else takes sixteen, as settling
//! and resolving once did. The bound is loose, so a slow or busy machine
//! does not fail it; it catches a stage becoming quadratic, not a few
//! percent. Before settling was fixed, it took 13.6 times as long here;
//! every stage now takes under 5.

mod support;

use std::time::{Duration, Instant};

use spit::{
    bind_dag, diagnose_checked_with_records, parse_input_spec, parse_pipeline, render_dag,
    render_source_inventory, resolve, Context, InputSource,
};
use support::Tree;

/// Subjects in the small dataset; the large one has four times as many.
const SUBJECTS: usize = 60;
/// How much longer the large dataset may take: 4 in proportion, 16 when
/// quadratic.
const MAX_GROWTH: f64 = 9.0;
/// Below this, a stage is too quick to time reliably, and too quick to matter.
const NOISE: Duration = Duration::from_millis(25);

const PIPELINE: &str = "\
path: out/{product}/{entities}.txt
source image : Image [sub, ses, run]
path image: sub-{sub}/ses-{ses}/image_run-{run}.nii
source mask : Mask [sub]
path mask: sub-{sub}/mask.nii
source reference : Reference [sub, ses]
path reference: sub-{sub}/ses-{ses}/reference.nii
operation clean(image: Image, mask: Mask) -> Image
operation align(image: Image, reference: Reference) -> Image
operation average(images: many Image) -> Image
operation compare(image: Image, reference: Reference) -> Score
cleaned = clean(image, mask)
aligned = align(cleaned, reference)
averaged = average(aligned @ vary(run))
score = compare(averaged, reference)
";

const RECIPE: &str = "\
discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}
require image count>=2 per [sub, ses]
require reference count=1 per [sub, ses]
drop [sub, ses] where image count<1
";

/// A dataset of `subjects` subjects, each with a mask and two sessions of a
/// reference and two runs.
fn dataset(subjects: usize) -> Tree {
    let mut files = Vec::new();
    for sub in 1..=subjects {
        files.push(format!("sub-{sub:04}/mask.nii"));
        for ses in 1..=2 {
            files.push(format!("sub-{sub:04}/ses-{ses}/reference.nii"));
            for run in 1..=2 {
                files.push(format!("sub-{sub:04}/ses-{ses}/image_run-{run}.nii"));
            }
        }
    }
    let files: Vec<_> = files.iter().map(String::as_str).collect();
    Tree::new("scaling", &files)
}

/// How long each stage takes over `subjects` subjects, at its quickest of
/// three runs.
fn stage_times(subjects: usize) -> Vec<(&'static str, Duration)> {
    let tree = dataset(subjects);
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let spec = parse_input_spec(RECIPE).unwrap();
    let mut best: Vec<(&'static str, Duration)> = Vec::new();
    for _ in 0..3 {
        let mut times = Vec::new();
        let mut time = |stage, start: Instant| times.push((stage, start.elapsed()));

        let start = Instant::now();
        let settled = spec
            .resolve(&pipeline, InputSource::Discover(tree.path()))
            .unwrap();
        time("settle", start);

        let start = Instant::now();
        let records = render_source_inventory(&settled.inventory, &pipeline, &spec.rules);
        time("write the .spitout", start);

        let start = Instant::now();
        let context = Context {
            path: None,
            recipe: Some(&spec),
            lenient: false,
        };
        assert!(diagnose_checked_with_records(PIPELINE, &records, context).is_ok());
        time("diagnose the records", start);

        let start = Instant::now();
        let dag = resolve(&pipeline, &settled.dag_inventory()).unwrap();
        time("resolve", start);
        assert_eq!(dag.jobs.len(), subjects * 12);

        let start = Instant::now();
        let spitdag = bind_dag(&pipeline, &dag).unwrap().to_json();
        time("write the .spitdag", start);
        assert!(spitdag.len() > subjects);

        let start = Instant::now();
        let text = render_dag(&dag);
        time("print the jobs", start);
        assert!(text.len() > subjects);

        if best.is_empty() {
            best = times;
        } else {
            for ((_, best), (_, took)) in best.iter_mut().zip(times) {
                *best = (*best).min(took);
            }
        }
    }
    best
}

#[test]
fn each_stage_grows_in_step_with_the_dataset() {
    let small = stage_times(SUBJECTS);
    let large = stage_times(SUBJECTS * 4);
    let mut slow = Vec::new();
    for ((stage, small), (_, large)) in small.iter().zip(&large) {
        let growth = large.as_secs_f64() / small.max(&Duration::from_micros(1)).as_secs_f64();
        eprintln!("{stage}: {small:?}, then {large:?}: {growth:.1} times as long");
        if *large > NOISE && growth > MAX_GROWTH {
            slow.push(format!(
                "{stage}: {small:?} for {SUBJECTS} subjects, {large:?} for {}, {growth:.1} times as long",
                SUBJECTS * 4
            ));
        }
    }
    assert!(
        slow.is_empty(),
        "stages that grew faster than the dataset:\n{}",
        slow.join("\n")
    );
}
