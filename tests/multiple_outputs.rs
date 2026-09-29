//! An operation with several outputs: one job owns them all, and the outputs
//! and verifications are checked before resolution.

use spit::{
    parse_pipeline, parse_source_inventory, render_dag, resolve, validate_commands, ResolvedDag,
};

/// The jobs with their bound paths, or the binding error as text.
fn bound(pipeline: &spit::Pipeline, dag: &spit::ResolvedDag) -> Result<String, String> {
    let bound = spit::bind_dag(pipeline, dag).map_err(|error| error.to_string())?;
    Ok(spit::render_bound_dag(&bound, true))
}

fn outputs(dag: &ResolvedDag) -> Vec<String> {
    dag.jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .map(ToString::to_string)
        .collect()
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
    assert!(bound(&pipeline, &dag)
        .unwrap()
        .contains("    csf: csf_fod[subject=a] : FOD\n      path: csf_fod/subject=a.txt\n"));
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
