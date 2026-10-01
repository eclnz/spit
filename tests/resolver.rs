mod support;

use spit::{
    parse_pipeline, render_dag, resolve, validate_pipeline, EntityBinding, InputBinding, InputPort,
    Invocation, OperationDef, Pipeline, ProductDef, ResolveError, ShapeRule, SourceInventory,
    SourceRecord, TypeExpr,
};

fn artifact(product: &str, pairs: &[(&str, &str)]) -> SourceRecord {
    SourceRecord::new(
        product,
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect::<EntityBinding>(),
    )
}

fn denoise_operation() -> OperationDef {
    OperationDef::new(
        "denoise",
        vec![InputPort::one("input", TypeExpr::named("Signal"))],
        TypeExpr::named("FilteredSignal"),
        ShapeRule::Preserve,
    )
}

fn register_operation() -> OperationDef {
    OperationDef::new(
        "register",
        vec![
            InputPort::one("moving", TypeExpr::named("FilteredSignal")),
            InputPort::one("reference", TypeExpr::named("Calibration")),
        ],
        TypeExpr::named("AlignedSignal"),
        ShapeRule::Preserve,
    )
}

fn registration_pipeline() -> Pipeline {
    Pipeline {
        products: vec![
            ProductDef::new(
                "denoised",
                TypeExpr::named("FilteredSignal"),
                ["site", "day", "run"],
            ),
            ProductDef::new(
                "calibration",
                TypeExpr::named("Calibration"),
                ["site", "day"],
            ),
            ProductDef::new(
                "registered",
                TypeExpr::named("AlignedSignal"),
                ["site", "day", "run"],
            ),
        ],
        operations: vec![register_operation()],
        invocations: vec![Invocation::new(
            "register",
            vec![
                InputBinding::product("denoised"),
                InputBinding::product("calibration"),
            ],
            "registered",
        )],
        ..Pipeline::default()
    }
}

#[test]
fn expands_one_to_one_over_two_runs() {
    let pipeline = Pipeline {
        products: vec![
            ProductDef::new("signal", TypeExpr::named("Signal"), ["site", "day", "run"]),
            ProductDef::new(
                "denoised",
                TypeExpr::named("FilteredSignal"),
                ["site", "day", "run"],
            ),
        ],
        operations: vec![denoise_operation()],
        invocations: vec![Invocation::new(
            "denoise",
            vec![InputBinding::product("signal")],
            "denoised",
        )],
        ..Pipeline::default()
    };
    let inventory = SourceInventory {
        artifacts: vec![
            artifact("signal", &[("site", "01"), ("day", "01"), ("run", "1")]),
            artifact("signal", &[("site", "01"), ("day", "01"), ("run", "2")]),
        ],
        ..SourceInventory::default()
    };

    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 2);
    assert_eq!(dag.artifact(dag.jobs[0].output()).product, "denoised");
    assert_eq!(
        dag.artifact(dag.jobs[0].output()).entities.get("run"),
        Some("1")
    );
    assert_eq!(
        dag.artifact(dag.jobs[1].output()).entities.get("run"),
        Some("2")
    );
}

#[test]
fn reuses_less_specific_t1_across_runs() {
    let pipeline = registration_pipeline();
    let inventory = SourceInventory {
        artifacts: vec![
            artifact("denoised", &[("site", "01"), ("day", "01"), ("run", "1")]),
            artifact("denoised", &[("site", "01"), ("day", "01"), ("run", "2")]),
            artifact("calibration", &[("site", "01"), ("day", "01")]),
        ],
        ..SourceInventory::default()
    };

    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 2);
    assert_eq!(dag.jobs[0].inputs[1], dag.jobs[1].inputs[1]);
    assert_eq!(
        dag.artifact(dag.jobs[0].output()).entities.get("run"),
        Some("1")
    );
    assert_eq!(
        dag.artifact(dag.jobs[1].output()).entities.get("run"),
        Some("2")
    );
}

#[test]
fn reports_missing_input() {
    let pipeline = registration_pipeline();
    let mut inventory = SourceInventory::default();
    inventory.artifacts.push(artifact(
        "denoised",
        &[("site", "01"), ("day", "02"), ("run", "1")],
    ));
    inventory
        .artifacts
        .push(artifact("calibration", &[("site", "01"), ("day", "01")]));

    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::MissingInput { site: spit::PortSite { port, .. }, .. }) if port == "reference"
    ));
}

#[test]
fn rejects_secondary_input_with_dimensions_absent_from_driver() {
    let mut pipeline = registration_pipeline();
    pipeline.products[1] = ProductDef::new(
        "calibration",
        TypeExpr::named("Calibration"),
        ["site", "day", "mode"],
    );
    let inventory = SourceInventory {
        artifacts: vec![
            artifact("denoised", &[("site", "01"), ("day", "01"), ("run", "1")]),
            artifact(
                "calibration",
                &[("site", "01"), ("day", "01"), ("mode", "A")],
            ),
            artifact(
                "calibration",
                &[("site", "01"), ("day", "01"), ("mode", "B")],
            ),
        ],
        ..SourceInventory::default()
    };

    for inventory in [inventory, SourceInventory::default()] {
        assert!(matches!(
            resolve(&pipeline, &inventory),
            Err(ResolveError::UnsupportedShapeRelationship { detail, .. })
                if detail.contains("input `calibration` has dimensions absent from driving product `denoised`: mode")
        ));
    }
}

#[test]
fn checks_types_before_concrete_expansion() {
    let mut pipeline = registration_pipeline();
    pipeline.products.push(ProductDef::new(
        "signal",
        TypeExpr::named("Signal"),
        ["site", "day", "run"],
    ));
    pipeline.invocations[0].inputs[1] = InputBinding::product("signal");

    assert!(matches!(
        resolve(&pipeline, &SourceInventory::default()),
        Err(ResolveError::TypeMismatch { site: spit::PortSite { port, .. }, .. }) if port == "reference"
    ));
}

#[test]
fn aggregates_each_fixed_dimension_group() {
    let pipeline = Pipeline {
        products: vec![
            ProductDef::new(
                "registered",
                TypeExpr::named("AlignedSignal"),
                ["site", "day", "run"],
            ),
            ProductDef::new(
                "mean_signal",
                TypeExpr::named("MeanSignal"),
                ["site", "day"],
            ),
        ],
        operations: vec![OperationDef::new(
            "mean",
            vec![InputPort::many("input", TypeExpr::named("AlignedSignal"))],
            TypeExpr::named("MeanSignal"),
            ShapeRule::Aggregate,
        )],
        invocations: vec![Invocation::new(
            "mean",
            vec![InputBinding::vary("registered", "run")],
            "mean_signal",
        )],
        ..Pipeline::default()
    };
    let mut inventory = SourceInventory::default();
    for site in ["01", "02"] {
        for run in ["1", "2"] {
            inventory.artifacts.push(artifact(
                "registered",
                &[("site", site), ("day", "01"), ("run", run)],
            ));
        }
    }

    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 2);
    assert!(dag
        .jobs
        .iter()
        .all(|job| job.input_artifacts().count() == 2));
    assert!(dag
        .jobs
        .iter()
        .all(|job| !dag.artifact(job.output()).entities.binds("run")));
    assert_ne!(
        dag.artifact(dag.jobs[0].output()).entities,
        dag.artifact(dag.jobs[1].output()).entities
    );
}

#[test]
fn rejects_accidental_cartesian_product() {
    let pipeline = Pipeline {
        products: vec![
            ProductDef::new("a", TypeExpr::named("A"), ["site", "run"]),
            ProductDef::new("b", TypeExpr::named("B"), ["site", "echo"]),
            ProductDef::new("c", TypeExpr::named("C"), ["site", "run"]),
        ],
        operations: vec![OperationDef::new(
            "combine",
            vec![
                InputPort::one("a", TypeExpr::named("A")),
                InputPort::one("b", TypeExpr::named("B")),
            ],
            TypeExpr::named("C"),
            ShapeRule::Preserve,
        )],
        invocations: vec![Invocation::new(
            "combine",
            vec![InputBinding::product("a"), InputBinding::product("b")],
            "c",
        )],
        ..Pipeline::default()
    };
    let mut inventory = SourceInventory::default();
    for run in ["1", "2"] {
        inventory
            .artifacts
            .push(artifact("a", &[("site", "01"), ("run", run)]));
    }
    for echo in ["1", "2"] {
        inventory
            .artifacts
            .push(artifact("b", &[("site", "01"), ("echo", echo)]));
    }

    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::UnsupportedShapeRelationship { detail, .. })
            if detail.contains("input `b` has dimensions absent from driving product `a`: echo")
    ));
}

#[test]
fn tracks_dependencies_through_full_pipeline() {
    let pipeline = full_pipeline();
    let dag = resolve(&pipeline, &full_inventory()).unwrap();
    assert_eq!(dag.jobs.len(), 5);
    assert_eq!(dag.jobs[2].dependencies, vec![1]);
    assert_eq!(dag.jobs[3].dependencies, vec![2]);
    assert_eq!(dag.jobs[4].dependencies, vec![3, 4]);
    let text = render_dag(&dag);
    assert!(text.contains("signal[site=01,day=01,run=1]"));
    assert!(text.contains("mean_signal[site=01,day=01]"));
}

#[test]
fn catches_duplicate_source_artifact() {
    let pipeline = full_pipeline();
    let mut inventory = full_inventory();
    inventory.artifacts.push(inventory.artifacts[0].clone());
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::DuplicateSourceArtifact { .. })
    ));
}

#[test]
fn catches_product_cycle() {
    let pipeline = Pipeline {
        products: vec![
            ProductDef::new("a", TypeExpr::named("A"), ["site"]),
            ProductDef::new("b", TypeExpr::named("A"), ["site"]),
        ],
        operations: vec![OperationDef::new(
            "copy",
            vec![InputPort::one("input", TypeExpr::named("A"))],
            TypeExpr::named("A"),
            ShapeRule::Preserve,
        )],
        invocations: vec![
            Invocation::new("copy", vec![InputBinding::product("b")], "a"),
            Invocation::new("copy", vec![InputBinding::product("a")], "b"),
        ],
        ..Pipeline::default()
    };
    assert!(matches!(
        resolve(&pipeline, &SourceInventory::default()),
        Err(ResolveError::Cycle { .. })
    ));
}

#[test]
fn source_inventory_changes_job_count_without_changing_pipeline() {
    let pipeline = full_pipeline();
    let one_run = SourceInventory {
        artifacts: vec![
            artifact("signal", &[("site", "01"), ("day", "01"), ("run", "1")]),
            artifact("calibration", &[("site", "01"), ("day", "01")]),
        ],
        ..SourceInventory::default()
    };
    assert_eq!(resolve(&pipeline, &one_run).unwrap().jobs.len(), 3);
    assert_eq!(resolve(&pipeline, &full_inventory()).unwrap().jobs.len(), 5);
}

fn full_pipeline() -> Pipeline {
    Pipeline {
        products: vec![
            ProductDef::new("signal", TypeExpr::named("Signal"), ["site", "day", "run"]),
            ProductDef::new(
                "calibration",
                TypeExpr::named("Calibration"),
                ["site", "day"],
            ),
            ProductDef::new(
                "denoised",
                TypeExpr::named("FilteredSignal"),
                ["site", "day", "run"],
            ),
            ProductDef::new(
                "registered",
                TypeExpr::named("AlignedSignal"),
                ["site", "day", "run"],
            ),
            ProductDef::new(
                "mean_signal",
                TypeExpr::named("MeanSignal"),
                ["site", "day"],
            ),
        ],
        operations: vec![
            denoise_operation(),
            register_operation(),
            OperationDef::new(
                "mean",
                vec![InputPort::many("input", TypeExpr::named("AlignedSignal"))],
                TypeExpr::named("MeanSignal"),
                ShapeRule::Aggregate,
            ),
        ],
        invocations: vec![
            Invocation::new("denoise", vec![InputBinding::product("signal")], "denoised"),
            Invocation::new(
                "register",
                vec![
                    InputBinding::product("denoised"),
                    InputBinding::product("calibration"),
                ],
                "registered",
            ),
            Invocation::new(
                "mean",
                vec![InputBinding::vary("registered", "run")],
                "mean_signal",
            ),
        ],
        ..Pipeline::default()
    }
}

fn full_inventory() -> SourceInventory {
    SourceInventory {
        artifacts: vec![
            artifact("signal", &[("site", "01"), ("day", "01"), ("run", "1")]),
            artifact("signal", &[("site", "01"), ("day", "01"), ("run", "2")]),
            artifact("calibration", &[("site", "01"), ("day", "01")]),
        ],
        ..SourceInventory::default()
    }
}

#[test]
fn named_ports_and_the_calls_aggregate_shape_are_checked() {
    let text = "source raw [site, run]\noperation combine(runs: many)\nresult = combine(raw @ vary(run))\nsources:\n  raw[site=01,run=2]\n  raw[site=01,run=1]\n";
    let (pipeline, inventory) = support::parse_fixture(text).unwrap();
    assert_eq!(pipeline.operations[0].inputs[0].name, "runs");
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    assert_eq!(dag.artifact(dag.jobs[0].output()).entities.len(), 1);

    let wrong_shape = text.replace("result =", "result : Data [site, run] =");
    let (pipeline, inventory) = support::parse_fixture(&wrong_shape).unwrap();
    assert!(resolve(&pipeline, &inventory.unwrap()).is_err());
}

#[test]
fn a_many_input_names_its_dimensions_at_the_call() {
    let missing = "source raw [site, run]\noperation combine(runs: many)\nresult = combine(raw)\n";
    let error = validate_pipeline(&parse_pipeline(missing).unwrap())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("many input `runs` needs `@ vary(dimension)`"),
        "{error}"
    );

    let dropped = "source raw [site, run]\noperation combine(runs: many) @ drop(run)\n";
    let error = parse_pipeline(dropped).unwrap_err();
    assert_eq!(error.line(), 2);
    assert!(
        error.message().contains("remove `@ drop(run)`")
            && error
                .message()
                .contains("`result = combine(input @ vary(run))`"),
        "{error}"
    );
}

#[test]
fn pipeline_checks_need_no_inventory() {
    let text = "source raw : Table [id]\noperation clean(Table) -> Table\n\ncleaned = clean(rwa)\n";
    assert_eq!(
        validate_pipeline(&parse_pipeline(text).unwrap()).unwrap_err(),
        ResolveError::UnknownProduct {
            name: "rwa".to_owned()
        }
    );

    let text = "source raw : Table [id]\nsource other : Other [id]\noperation clean(Table) -> Table\ncleaned = clean(other)\n";
    assert!(matches!(
        validate_pipeline(&parse_pipeline(text).unwrap()),
        Err(ResolveError::TypeMismatch { .. })
    ));

    let (pipeline, inventory) = support::parse_fixture(include_str!(
        "../examples/commands/command_demo/command_demo.spit"
    ))
    .unwrap();
    assert!(inventory.is_none());
    validate_pipeline(&pipeline).unwrap();
}
