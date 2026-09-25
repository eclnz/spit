use spit::{
    render_dag, resolve, CountRequirement, CoverageRule, EntityBinding, InputBinding, InputPort,
    Invocation, OperationDef, Pipeline, ProductDef, ResolveError, ShapeRule, SourceInventory,
    SourceRecord,
};

fn artifact(product: &str, pairs: &[(&str, &str)]) -> SourceRecord {
    SourceRecord::new(
        product,
        EntityBinding(
            pairs
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
        ),
    )
}

fn denoise_operation() -> OperationDef {
    OperationDef::new(
        "denoise",
        vec![InputPort::one("input", "BOLD")],
        "DenoisedBOLD",
        ShapeRule::Preserve,
    )
}

fn register_operation() -> OperationDef {
    OperationDef::new(
        "register",
        vec![
            InputPort::one("moving", "DenoisedBOLD"),
            InputPort::one("reference", "T1w"),
        ],
        "RegisteredBOLD",
        ShapeRule::Preserve,
    )
}

fn registration_pipeline() -> Pipeline {
    Pipeline {
        products: vec![
            ProductDef::new("denoised", "DenoisedBOLD", &["sub", "ses", "run"]),
            ProductDef::new("t1w", "T1w", &["sub", "ses"]),
            ProductDef::new("registered", "RegisteredBOLD", &["sub", "ses", "run"]),
        ],
        operations: vec![register_operation()],
        invocations: vec![Invocation::new(
            "register",
            vec![
                InputBinding::product("denoised"),
                InputBinding::product("t1w"),
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
            ProductDef::new("bold", "BOLD", &["sub", "ses", "run"]),
            ProductDef::new("denoised", "DenoisedBOLD", &["sub", "ses", "run"]),
        ],
        operations: vec![denoise_operation()],
        invocations: vec![Invocation::new(
            "denoise",
            vec![InputBinding::product("bold")],
            "denoised",
        )],
        ..Pipeline::default()
    };
    let inventory = SourceInventory {
        artifacts: vec![
            artifact("bold", &[("sub", "01"), ("ses", "01"), ("run", "1")]),
            artifact("bold", &[("sub", "01"), ("ses", "01"), ("run", "2")]),
        ],
        ..SourceInventory::default()
    };

    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 2);
    assert_eq!(dag.jobs[0].output.product, "denoised");
    assert_eq!(dag.jobs[0].output.entities.0["run"], "1");
    assert_eq!(dag.jobs[1].output.entities.0["run"], "2");
}

#[test]
fn reuses_less_specific_t1_across_runs() {
    let pipeline = registration_pipeline();
    let inventory = SourceInventory {
        artifacts: vec![
            artifact("denoised", &[("sub", "01"), ("ses", "01"), ("run", "1")]),
            artifact("denoised", &[("sub", "01"), ("ses", "01"), ("run", "2")]),
            artifact("t1w", &[("sub", "01"), ("ses", "01")]),
        ],
        ..SourceInventory::default()
    };

    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 2);
    assert_eq!(dag.jobs[0].inputs[1], dag.jobs[1].inputs[1]);
    assert_eq!(dag.jobs[0].output.entities.0["run"], "1");
    assert_eq!(dag.jobs[1].output.entities.0["run"], "2");
}

#[test]
fn reports_missing_input() {
    let pipeline = registration_pipeline();
    let mut inventory = SourceInventory::default();
    inventory.artifacts.push(artifact(
        "denoised",
        &[("sub", "01"), ("ses", "02"), ("run", "1")],
    ));
    inventory
        .artifacts
        .push(artifact("t1w", &[("sub", "01"), ("ses", "01")]));

    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::MissingInput { port, .. }) if port == "reference"
    ));
}

#[test]
fn reports_ambiguous_input_within_named_family() {
    let mut pipeline = registration_pipeline();
    pipeline.products[1] = ProductDef::new("t1w", "T1w", &["sub", "ses", "acq"]);
    let inventory = SourceInventory {
        artifacts: vec![
            artifact("denoised", &[("sub", "01"), ("ses", "01"), ("run", "1")]),
            artifact("t1w", &[("sub", "01"), ("ses", "01"), ("acq", "A")]),
            artifact("t1w", &[("sub", "01"), ("ses", "01"), ("acq", "B")]),
        ],
        ..SourceInventory::default()
    };

    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::AmbiguousInput { port, candidates, .. })
            if port == "reference" && candidates.len() == 2
    ));
}

#[test]
fn checks_types_before_concrete_expansion() {
    let mut pipeline = registration_pipeline();
    pipeline
        .products
        .push(ProductDef::new("bold", "BOLD", &["sub", "ses", "run"]));
    pipeline.invocations[0].inputs[1] = InputBinding::product("bold");

    assert!(matches!(
        resolve(&pipeline, &SourceInventory::default()),
        Err(ResolveError::TypeMismatch { port, .. }) if port == "reference"
    ));
}

#[test]
fn aggregates_each_fixed_dimension_group() {
    let pipeline = Pipeline {
        products: vec![
            ProductDef::new("registered", "RegisteredBOLD", &["sub", "ses", "run"]),
            ProductDef::new("mean_bold", "MeanBOLD", &["sub", "ses"]),
        ],
        operations: vec![OperationDef::new(
            "mean",
            vec![InputPort::many("input", "RegisteredBOLD")],
            "MeanBOLD",
            ShapeRule::Aggregate,
        )],
        invocations: vec![Invocation::new(
            "mean",
            vec![InputBinding::vary("registered", "run")],
            "mean_bold",
        )],
        ..Pipeline::default()
    };
    let mut inventory = SourceInventory::default();
    for sub in ["01", "02"] {
        for run in ["1", "2"] {
            inventory.artifacts.push(artifact(
                "registered",
                &[("sub", sub), ("ses", "01"), ("run", run)],
            ));
        }
    }

    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 2);
    assert!(dag.jobs.iter().all(|job| job.inputs.len() == 2));
    assert!(dag
        .jobs
        .iter()
        .all(|job| !job.output.entities.0.contains_key("run")));
    assert_ne!(dag.jobs[0].output.entities, dag.jobs[1].output.entities);
}

#[test]
fn rejects_accidental_cartesian_product() {
    let pipeline = Pipeline {
        products: vec![
            ProductDef::new("a", "A", &["sub", "run"]),
            ProductDef::new("b", "B", &["sub", "echo"]),
            ProductDef::new("c", "C", &["sub", "run"]),
        ],
        operations: vec![OperationDef::new(
            "combine",
            vec![InputPort::one("a", "A"), InputPort::one("b", "B")],
            "C",
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
            .push(artifact("a", &[("sub", "01"), ("run", run)]));
    }
    for echo in ["1", "2"] {
        inventory
            .artifacts
            .push(artifact("b", &[("sub", "01"), ("echo", echo)]));
    }

    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::AmbiguousInput { port, .. }) if port == "b"
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
    assert!(text.contains("bold[sub=01,ses=01,run=1]"));
    assert!(text.contains("mean_bold[sub=01,ses=01]"));
}

#[test]
fn catches_duplicate_source_artifact() {
    let pipeline = full_pipeline();
    let mut inventory = full_inventory();
    inventory.artifacts.push(inventory.artifacts[0].clone());
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::DuplicateOutputArtifact { .. })
    ));
}

#[test]
fn catches_product_cycle() {
    let pipeline = Pipeline {
        products: vec![
            ProductDef::new("a", "A", &["sub"]),
            ProductDef::new("b", "A", &["sub"]),
        ],
        operations: vec![OperationDef::new(
            "copy",
            vec![InputPort::one("input", "A")],
            "A",
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
fn coverage_checks_each_observed_context_without_a_global_count() {
    let pipeline = Pipeline {
        products: vec![ProductDef::new("image", "Image", &["subject", "visit"])],
        constraints: vec![CoverageRule::new(
            "image",
            &["subject", "visit"],
            CountRequirement::Exactly(1),
        )],
        ..Pipeline::default()
    };
    let inventory = SourceInventory {
        artifacts: vec![artifact("image", &[("subject", "A"), ("visit", "1")])],
        contexts: vec![
            EntityBinding::from_pairs([("subject", "A"), ("visit", "1")]),
            EntityBinding::from_pairs([("subject", "B"), ("visit", "1")]),
        ],
    };
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::CoverageViolation { product, found: 0, .. }) if product == "image"
    ));

    let complete = SourceInventory {
        artifacts: vec![
            artifact("image", &[("subject", "A"), ("visit", "1")]),
            artifact("image", &[("subject", "B"), ("visit", "1")]),
        ],
        contexts: inventory.contexts,
    };
    assert!(resolve(&pipeline, &complete).is_ok());
}

#[test]
fn source_inventory_changes_job_count_without_changing_pipeline() {
    let pipeline = full_pipeline();
    let one_run = SourceInventory {
        artifacts: vec![
            artifact("bold", &[("sub", "01"), ("ses", "01"), ("run", "1")]),
            artifact("t1w", &[("sub", "01"), ("ses", "01")]),
        ],
        ..SourceInventory::default()
    };
    assert_eq!(resolve(&pipeline, &one_run).unwrap().jobs.len(), 3);
    assert_eq!(resolve(&pipeline, &full_inventory()).unwrap().jobs.len(), 5);
}

fn full_pipeline() -> Pipeline {
    Pipeline {
        products: vec![
            ProductDef::new("bold", "BOLD", &["sub", "ses", "run"]),
            ProductDef::new("t1w", "T1w", &["sub", "ses"]),
            ProductDef::new("denoised", "DenoisedBOLD", &["sub", "ses", "run"]),
            ProductDef::new("registered", "RegisteredBOLD", &["sub", "ses", "run"]),
            ProductDef::new("mean_bold", "MeanBOLD", &["sub", "ses"]),
        ],
        operations: vec![
            denoise_operation(),
            register_operation(),
            OperationDef::new(
                "mean",
                vec![InputPort::many("input", "RegisteredBOLD")],
                "MeanBOLD",
                ShapeRule::Aggregate,
            ),
        ],
        invocations: vec![
            Invocation::new("denoise", vec![InputBinding::product("bold")], "denoised"),
            Invocation::new(
                "register",
                vec![
                    InputBinding::product("denoised"),
                    InputBinding::product("t1w"),
                ],
                "registered",
            ),
            Invocation::new(
                "mean",
                vec![InputBinding::vary("registered", "run")],
                "mean_bold",
            ),
        ],
        constraints: Vec::new(),
    }
}

fn full_inventory() -> SourceInventory {
    SourceInventory {
        artifacts: vec![
            artifact("bold", &[("sub", "01"), ("ses", "01"), ("run", "1")]),
            artifact("bold", &[("sub", "01"), ("ses", "01"), ("run", "2")]),
            artifact("t1w", &[("sub", "01"), ("ses", "01")]),
        ],
        ..SourceInventory::default()
    }
}
