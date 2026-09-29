use spit::{parse_pipeline, parse_source_inventory, parse_type_expr, resolve, Job, ResolveError};

fn type_lab() -> (spit::Pipeline, spit::SourceInventory) {
    (
        parse_pipeline(include_str!("../examples/stress/type_lab.spit")).unwrap(),
        parse_source_inventory(include_str!("../examples/stress/type_lab.spitout")).unwrap(),
    )
}

fn jobs<'a>(dag: &'a spit::ResolvedDag, product: &str) -> Vec<&'a Job> {
    dag.jobs
        .iter()
        .filter(|job| job.output().product == product)
        .collect()
}

fn type_of(dag: &spit::ResolvedDag, product: &str) -> spit::TypeExpr {
    let family = jobs(dag, product);
    assert!(!family.is_empty(), "no jobs for {product}");
    let expected = family[0].output().artifact_type.clone();
    assert!(family
        .iter()
        .all(|job| job.output().artifact_type == expected));
    expected
}

#[test]
fn nested_types_propagate_across_polymorphic_branches_and_rollups() {
    let (pipeline, inventory) = type_lab();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 129);
    assert_eq!(jobs(&dag, "paired").len(), 6);
    assert_eq!(jobs(&dag, "selected_batch").len(), 4);
    assert_eq!(jobs(&dag, "rig_summary").len(), 3);
    assert_eq!(jobs(&dag, "lab_summary").len(), 2);
    assert_eq!(jobs(&dag, "global_summary").len(), 1);

    for (product, expected) in [
        ("normalized_lidar", "Stream<Frame<Lidar,Sensor>,Normalized>"),
        ("world_lidar", "Stream<Frame<Lidar,World>,Normalized>"),
        ("world_camera", "Stream<Frame<Camera,World>,Normalized>"),
        (
            "normalized_unclassified",
            "Stream<Frame<Unknown,Sensor>,Normalized>",
        ),
        (
            "world_unclassified",
            "Stream<Frame<Camera,World>,Normalized>",
        ),
        (
            "verified_unclassified",
            "Verified<Frame<Camera,World>,Normalized>",
        ),
        ("classified_opaque", "Classified<Camera,World,Unknown>"),
        (
            "verified_camera",
            "Verified<Frame<Camera,World>,Normalized>",
        ),
        ("raw_camera_quality", "Quality<Camera,Sensor,Raw>"),
        ("world_camera_quality", "Quality<Camera,World,Normalized>"),
        (
            "paired",
            "Pair<Frame<Lidar,World>,Frame<Camera,World>,Normalized>",
        ),
        ("fused", "Fused<World,Normalized>"),
        (
            "case_file",
            "Case<Bundle<Frame<Lidar,World>,Frame<Camera,World>>,Score<World,Normalized>>",
        ),
        ("decision", "Decision<World,Normalized>"),
        (
            "lidar_quality_batch",
            "QualityBatch<Lidar,World,Normalized>",
        ),
        ("audited", "Audit<World,Normalized>"),
        ("global_summary", "GlobalSummary<World,Normalized>"),
    ] {
        assert_eq!(
            type_of(&dag, product),
            parse_type_expr(expected, false).unwrap()
        );
    }

    let batch = jobs(&dag, "selected_batch")
        .into_iter()
        .find(|job| {
            job.output().entities.0.get("lab").map(String::as_str) == Some("Alpha")
                && job.output().entities.0.get("rig").map(String::as_str) == Some("R1")
                && job.output().entities.0.get("capture").map(String::as_str) == Some("C1")
        })
        .unwrap();
    assert_eq!(batch.inputs[0].len(), 2);
    assert_eq!(batch.dependencies.len(), 2);
    assert!(batch
        .input_artifacts()
        .all(|input| input.entities.0.get("capture") == Some(&"C1".to_owned())));
    assert_eq!(jobs(&dag, "global_summary")[0].dependencies.len(), 2);
}

#[test]
fn observatory_resolves_each_scope_and_reuses_generic_evidence() {
    let pipeline = parse_pipeline(include_str!("../examples/stress/observatory.spit")).unwrap();
    let inventory =
        parse_source_inventory(include_str!("../examples/stress/observatory.spitout")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 153);
    assert_eq!(jobs(&dag, "channel_signal").len(), 7);
    assert_eq!(jobs(&dag, "window_signal").len(), 6);
    assert_eq!(jobs(&dag, "device_evidence").len(), 4);
    assert_eq!(jobs(&dag, "site_evidence").len(), 3);
    assert_eq!(jobs(&dag, "org_evidence").len(), 2);
    assert_eq!(jobs(&dag, "global_portfolio").len(), 1);
    for (product, expected) in [
        ("device_evidence", "Evidence<Device>"),
        ("site_evidence", "Evidence<Site>"),
        ("org_evidence", "Evidence<Org>"),
        ("global_portfolio", "Approved<Portfolio>"),
    ] {
        assert_eq!(
            type_of(&dag, product),
            parse_type_expr(expected, false).unwrap()
        );
    }
}

#[test]
fn wrong_modality_calibration_fails_after_inferred_type_flows_downstream() {
    let text = include_str!("../examples/stress/type_lab.spit").replace(
        "world_lidar = project(normalized_lidar, lidar_cal)",
        "world_lidar = project(normalized_lidar, camera_cal)",
    );
    let pipeline = parse_pipeline(&text).unwrap();
    let inventory = type_lab().1;
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::TypeVariableConflict { operation, port, conflict, .. })
            if operation == "project" && port == "calibration" && conflict.variable == "Kind"
    ));
}

#[test]
fn nested_pair_rejects_a_second_lidar_stream() {
    let text = include_str!("../examples/stress/type_lab.spit").replace(
        "paired = pair(verified_lidar, verified_camera)",
        "paired = pair(verified_lidar, verified_lidar)",
    );
    let pipeline = parse_pipeline(&text).unwrap();
    let inventory = type_lab().1;
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::TypeMismatch { operation, port, .. })
            if operation == "pair" && port == "right"
    ));
}

#[test]
fn declared_nested_output_cannot_override_inferred_reference_space() {
    let text = include_str!("../examples/stress/type_lab.spit").replace(
        "world_lidar = project(normalized_lidar, lidar_cal)",
        "world_lidar : Stream<Frame<Lidar,Mars>,Normalized> [lab, rig, capture, slice] = project(normalized_lidar, lidar_cal)",
    );
    let pipeline = parse_pipeline(&text).unwrap();
    let inventory = type_lab().1;
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::TypeVariableConflict { operation, port, conflict, .. })
            if operation == "project" && port == "output" && conflict.variable == "TargetSpace"
    ));
}

#[test]
fn missing_peer_at_one_slice_does_not_cross_join_another_capture() {
    let sources = include_str!("../examples/stress/type_lab.spitout")
        .replace("    camera[lab=Alpha,rig=R1,capture=C1,slice=02]\n", "");
    let inventory = parse_source_inventory(&sources).unwrap();
    let pipeline = type_lab().0;
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::MissingInput { operation, port, .. })
            if operation == "pair" && port == "right"
    ));
}

#[test]
fn inferred_intermediate_type_mismatch_fails_even_with_no_artifacts() {
    let text = "source raw : A<Native> [id]\n\
                operation first(A<X>) -> B<X>\n\
                middle = first(raw)\n\
                operation second(B<Standard>) -> C\n\
                final = second(middle)\n";
    let pipeline = parse_pipeline(text).unwrap();
    for sources in ["sources:\n", "sources:\n    raw[id=one]\n"] {
        let inventory = parse_source_inventory(sources).unwrap();
        assert!(matches!(
            resolve(&pipeline, &inventory),
            Err(ResolveError::TypeMismatch { operation, port, .. })
                if operation == "second" && port == "input"
        ));
    }
}
