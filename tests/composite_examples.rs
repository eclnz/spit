//! The acceptance test of operations carried out by steps (#53), on the
//! examples under `examples/composites/`: from a short imported call,
//! every job it expands to can be inspected, and an error can be followed
//! back to the call and the exact line of the library's body. What the CLI
//! prints is compared byte for byte with `tests/fixtures/outputs/`; run
//! with `SPIT_BLESS=1` to write it again, and review the diff.

mod support;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const MRTRIX: &str = "examples/composites/mrtrix";
const GERMLINE: &str = "examples/composites/germline";

/// `spit` run in `folder` with `args`, and `stdin` on its standard input:
/// its exit code, standard output and standard error, with the folder's
/// absolute path written as `<example>`.
fn run(folder: &str, args: &[&str], stdin: Option<&str>) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .current_dir(folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.unwrap_or_default().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let text = format!(
        "$ spit {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
        args.join(" "),
        output.status.code().unwrap_or(-1),
        support::text(&output.stdout),
        support::text(&output.stderr),
    );
    // The `.spitdag` records the dataset's absolute folder.
    let folder = Path::new(folder).canonicalize().unwrap();
    text.replace(&folder.display().to_string(), "<example>")
}

/// The jobs of each operation in `counts`, the text `dag --counts` prints,
/// with any import prefix dropped.
fn jobs_by_operation(counts: &str) -> BTreeMap<String, usize> {
    let mut jobs = BTreeMap::new();
    for line in counts.lines() {
        let Some((count, step)) = line.trim_start().split_once("  ") else {
            continue;
        };
        let (Ok(count), Some((_, operation))) = (count.parse::<usize>(), step.split_once(" = "))
        else {
            continue;
        };
        let operation = operation.split_whitespace().next().unwrap_or_default();
        let operation = operation.rsplit("::").next().unwrap_or_default();
        *jobs.entry(operation.to_owned()).or_default() += count;
    }
    jobs
}

#[test]
fn every_job_of_a_call_can_be_inspected() {
    let runs: [&[&str]; 2] = [
        &["dag", "act.spitin", "--counts"],
        &["dag", "act.spitin", "--commands"],
    ];
    let text: String = runs.iter().map(|args| run(MRTRIX, args, None)).collect();
    support::check_output("composites_mrtrix", &text);

    let runs: [&[&str]; 3] = [
        &["dag", "somatic.spitin", "--counts"],
        &["dag", "somatic.spitin", "--commands"],
        &["dag", "somatic.spitin", "--json"],
    ];
    let text: String = runs.iter().map(|args| run(GERMLINE, args, None)).collect();
    support::check_output("composites_germline", &text);
}

#[test]
fn a_call_makes_the_jobs_the_steps_it_replaces_made() {
    // The session call makes the 48 jobs of the ACT example's hand-written
    // `preprocess` stage, the same number of each operation.
    let output = support::spit(&[
        "dag",
        "examples/commands/mrtrix3_act/mrtrix3_act.spitin",
        "--counts",
    ]);
    let written = support::text(&output.stdout);
    let preprocess: String = written
        .lines()
        .filter(|line| line.trim_end().ends_with("preprocess") || line.contains(" preprocess/"))
        .map(|line| format!("{line}\n"))
        .collect();
    let output = support::spit(&["dag", &format!("{MRTRIX}/act.spitin"), "--counts"]);
    let called = support::text(&output.stdout);
    let call: String = called
        .lines()
        .skip_while(|line| !line.contains("clean_dwi_session"))
        .take_while(|line| !line.contains("in this call"))
        .map(|line| format!("{line}\n"))
        .collect();
    let preprocess = jobs_by_operation(&preprocess);
    assert_eq!(preprocess.values().sum::<usize>(), 48, "{written}");
    assert_eq!(jobs_by_operation(&call), preprocess, "{called}");
}

#[test]
fn an_error_leads_back_to_the_call_and_the_line_of_the_body() {
    // Without the reverse b=0 of one session, the call cannot be planned:
    // the error is at the argument the caller gave, and points into the
    // library at the step that reads it.
    let spitout = std::fs::read_to_string(Path::new(MRTRIX).join("act.spitout")).unwrap();
    let gap = spitout
        .lines()
        .filter(|line| !line.contains("reverse_b0[sub=02,ses=01]"))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    assert_ne!(gap, spitout);
    let mut text = run(MRTRIX, &["dag", "act.spit", "-"], Some(&gap));
    // A type error: `t1w` given where the session's DWI goes.
    let pipeline = std::fs::read_to_string(Path::new(MRTRIX).join("act.spit")).unwrap();
    let swapped = pipeline.replace("clean_dwi_session(raw_dwi,", "clean_dwi_session(t1w,");
    assert_ne!(swapped, pipeline);
    text.push_str(&run(
        MRTRIX,
        &["check", "act.spit", "--stdin"],
        Some(&swapped),
    ));
    text.push_str(&run(
        MRTRIX,
        &["check", "act.spit", "--stdin", "--json"],
        Some(&swapped),
    ));
    support::check_output("composites_errors", &text);
    assert!(text.contains("\n  --> mrtrix_dwi.spit: line 45, column 15: the step in the body of `mrx::clean_dwi_session`\n"), "{text}");
}

#[test]
fn two_calls_share_no_file() {
    let output = support::spit(&["dag", &format!("{GERMLINE}/somatic.spitin"), "--paths"]);
    let jobs = support::text(&output.stdout);
    assert!(output.status.success(), "{}", support::text(&output.stderr));
    for call in ["normal_bam", "tumour_bam"] {
        for lane in [1, 2] {
            let path = format!("path: out/{call}.aligned/patient=P01__lane={lane}.sam\n");
            assert!(jobs.contains(&path), "{path} in {jobs}");
        }
    }
}

#[test]
fn checks_add_up_on_the_step_that_makes_an_output() {
    let output = support::spit(&["dag", &format!("{GERMLINE}/somatic.spitin"), "--commands"]);
    let commands = support::text(&output.stdout);
    let job = commands
        .split("\n\n")
        .find(|job| job.contains("-O out/normal_bam/patient=P01.bam"))
        .unwrap();
    // `mark_duplicates`'s own check, and the one `align_sample` adds.
    assert!(job.ends_with(
        "  check:  test -s out/normal_bam/patient=P01.bam\n  check:  samtools quickcheck out/normal_bam/patient=P01.bam"
    ), "{job}");
}

#[test]
fn an_import_alone_adds_no_jobs() {
    let tree = support::Tree::new("composite-import-only", &["fastq/reference/genome.fa"]);
    let library = std::fs::read_to_string(Path::new(GERMLINE).join("germline.spit")).unwrap();
    tree.write("germline.spit", &library);
    tree.write(
        "only.spit",
        "use align_sample from germline.spit as gl\nsource reference : Fasta .fa\npath reference: fastq/reference/genome.fa\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["dag", "only.spit", "--root", ".", "--counts"])
        .current_dir(tree.path())
        .output()
        .unwrap();
    assert_eq!(
        support::text(&output.stdout),
        "jobs  step\n   0  total\n",
        "{}",
        support::text(&output.stderr)
    );
}
