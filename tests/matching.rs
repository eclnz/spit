//! How a step's inputs are matched: many-inputs, selectors (`where`, `same`),
//! broadcasts (`each`), the driving input, declared order, and `min`.

mod support;

use support::outputs;

use spit::{parse_pipeline, parse_source_inventory, resolve, ResolveError, ResolvedDag};

fn resolve_text(text: &str, inventory: &str) -> Result<ResolvedDag, ResolveError> {
    let pipeline = parse_pipeline(text).unwrap();
    resolve(&pipeline, &parse_source_inventory(inventory).unwrap())
}

const COMBINE: &str = "\
source result [site, run]
source policy [site]
operation combine(results: many Result, policy: Policy) -> Summary
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
    assert_eq!(
        dag.artifact(dag.jobs[0].inputs[1][0]).to_string(),
        "policy[site=A]"
    );
    assert_eq!(outputs(&dag), ["summary[site=A]", "summary[site=B]"]);
}

#[test]
fn one_aggregate_can_vary_two_dimensions_in_product_order() {
    let pipeline = "source summary [model, config]\noperation leaderboard(summaries: many Summary) -> Table @ min(3)\noverall = leaderboard(summary @ vary(model, config))\n";
    let inventory = "sources:\n  summary[model=b,config=1]\n  summary[model=a,config=10]\n  summary[model=a,config=2]\n";
    let dag = resolve_text(pipeline, inventory).unwrap();
    assert_eq!(dag.jobs.len(), 1);
    let job = &dag.jobs[0];
    let members: Vec<_> = job.inputs[0]
        .iter()
        .map(|id| dag.artifact(*id).to_string())
        .collect();
    assert_eq!(
        members,
        [
            "summary[config=2,model=a]",
            "summary[config=10,model=a]",
            "summary[config=1,model=b]"
        ]
    );
    assert_eq!(dag.artifact(job.output()).to_string(), "overall");
    assert!(resolve_text(
        &pipeline.replace("vary(model, config)", "vary(config, model)"),
        inventory
    )
    .is_ok());

    let too_small = pipeline.replace("min(3)", "min(4)");
    assert!(matches!(
        resolve_text(&too_small, inventory),
        Err(ResolveError::CollectionTooSmall {
            found: 3,
            minimum: 4,
            ..
        })
    ));
    for invalid in [
        pipeline.replace("vary(model, config)", "vary(model) @ vary(config)"),
        pipeline.replace("vary(model, config)", "vary(model, model)"),
    ] {
        assert!(parse_pipeline(&invalid).is_err(), "{invalid}");
    }
}

#[test]
fn a_call_varies_one_or_several_dimensions() {
    for (source_dimensions, vary, inventory) in [
        (
            "site, run",
            "run",
            "sources:\n  result[site=A,run=1]\n  result[site=A,run=2]\n",
        ),
        (
            "site, model, config",
            "model, config",
            "sources:\n  result[site=A,model=b,config=1]\n  result[site=A,model=a,config=2]\n",
        ),
    ] {
        let text = format!(
            "source result [{source_dimensions}]\noperation combine(items: many Result) -> Summary\nsummary = combine(result @ vary({vary}))\n"
        );
        let pipeline = parse_pipeline(&text).unwrap();
        assert_eq!(
            pipeline.invocations[0].inputs[0].vary,
            vary.split(", ").collect::<Vec<_>>()
        );
        assert_eq!(pipeline.products[1].dimensions, ["site"]);
        let dag = resolve(&pipeline, &parse_source_inventory(inventory).unwrap()).unwrap();
        assert_eq!(dag.jobs.len(), 1);
        assert_eq!(dag.jobs[0].inputs[0].len(), 2);
        assert_eq!(
            dag.artifact(dag.jobs[0].output()).to_string(),
            "summary[site=A]"
        );
    }
}

#[test]
fn a_single_input_of_an_aggregate_must_match_each_group() {
    let missing = resolve_text(COMBINE, "sources:\n  result[site=A,run=1]\n");
    assert!(matches!(
        missing,
        Err(ResolveError::MissingInput { site: spit::PortSite { port, product, .. }, .. }) if port == "policy" && product == "policy"
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
        error.message().contains("at most one `many` input"),
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
        dag.artifact(dag.jobs[0].inputs[1][0]).to_string(),
        "calibration[revision=2,site=A]"
    );
    assert_eq!(outputs(&dag), ["calibrated[run=1,site=A]"]);
}

#[test]
fn where_filters_the_driver_and_removes_its_dimension_from_the_output() {
    let text = "source image [site, echo]\noperation keep(image: Image) -> Image\nfirst = keep(image @ where(echo=1))\n";
    let (pipeline, _) = support::parse_fixture(text).unwrap();
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
    let text = "source frame [subject, acq, run]\noperation stack(frames: many Frame) -> Stack\nstacked = stack(frame @ where(acq=fast) @ vary(run))\n";
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
        Err(ResolveError::AmbiguousInput { site: spit::PortSite { port, .. }, .. }) if port == "calibration"
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
        error.message().contains("`@ where(dimension=value, ...)`"),
        "{error}"
    );
}

const PREDICT: &str = "\
dimensions [station, scenario]
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
    let (pipeline, _) = support::parse_fixture(&text).unwrap();
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
        dag.artifact(dag.jobs[1].inputs[2][0]).to_string(),
        "parameters[scenario=high]"
    );
    let missing = resolve_text(
        &text,
        &SCENARIOS.replace("  parameters[scenario=high]\n", ""),
    );
    assert!(matches!(
        missing,
        Err(ResolveError::MissingInput { site: spit::PortSite { port, .. }, context, .. })
            if port == "parameters" && context.to_string().contains("scenario=high")
    ));
}

#[test]
fn a_broadcast_dimension_can_be_collected_again() {
    let text = "\
dimensions [station, rep]
source reading [station]
source seed [rep]
operation simulate(reading: Series, seed: Seed) -> Series
trial = simulate(reading, seed @ each(rep))
operation average(items: many Series) -> Series
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
operation stack(items: many Series, model: Model) -> Stack
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
    assert!(
        error.message().contains("names `scenario` twice"),
        "{error}"
    );
}

#[test]
fn port_order_does_not_decide_the_driving_input() {
    let text = "\
source signal [site, run]
source calibration [site]
operation apply(calibration: Calibration, data: Signal) -> Signal
calibrated = apply(calibration, signal)
";
    let (pipeline, _) = support::parse_fixture(text).unwrap();
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
operation stack(frames: many Frame) -> Stack
stacked = stack(frame @ vary(run))
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
        .map(|&frame| dag.artifact(frame).entities.get("run").unwrap().to_owned())
        .collect();
    assert_eq!(runs, ["1", "2", "10"]);
}

#[test]
fn min_rejects_a_collection_that_is_too_small() {
    let text = "source frame [subject, run]\noperation stack(frames: many Frame) -> Stack @ min(2)\nstacked = stack(frame @ vary(run))\n";
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
            "operation f(a: A) -> B @ min(2)",
            "`@ min(count)` requires a many input",
        ),
        (
            "operation f(as: many A) -> B @ min(0)",
            "needs a positive integer",
        ),
    ] {
        let error = parse_pipeline(&format!("{declaration}\n")).unwrap_err();
        assert!(error.message().contains(expected), "{error}");
    }
}
