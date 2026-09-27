use spit::{
    parse_document, parse_pipeline, parse_source_inventory, parse_type_expr, resolve,
    Compatibility, ResolveError, Substitutions, TypeExpr, TypeUnifyError,
};

fn product(text: &str) -> TypeExpr {
    parse_type_expr(text, false).unwrap()
}

fn signature(text: &str) -> TypeExpr {
    parse_type_expr(text, true).unwrap()
}

#[test]
fn exact_named_types_match() {
    let mut substitutions = Substitutions::default();
    assert_eq!(
        substitutions.unify(&product("Signal"), &product("Signal")),
        Ok(Compatibility::Compatible)
    );
}

#[test]
fn different_named_types_fail() {
    let mut substitutions = Substitutions::default();
    assert!(matches!(
        substitutions.unify(&product("Signal"), &product("Calibration")),
        Err(TypeUnifyError::Mismatch { .. })
    ));
}

#[test]
fn different_constructors_and_arities_fail() {
    let mut substitutions = Substitutions::default();
    assert!(matches!(
        substitutions.unify(&signature("Signal<S>"), &product("Image<Native>")),
        Err(TypeUnifyError::Mismatch { .. })
    ));
    assert!(matches!(
        substitutions.unify(&signature("Pair<A,B>"), &product("Pair<Native>")),
        Err(TypeUnifyError::Mismatch { .. })
    ));
}

#[test]
fn generic_input_infers_variable_and_substitutes_output() {
    let mut substitutions = Substitutions::default();
    assert_eq!(
        substitutions.unify(&signature("Signal<S>"), &product("Signal<Native>")),
        Ok(Compatibility::Compatible)
    );
    assert_eq!(substitutions.0["S"], product("Native"));
    assert_eq!(
        substitutions.substitute(&signature("FilteredSignal<S>")),
        product("FilteredSignal<Native>")
    );
}

#[test]
fn generic_import_preserves_image_space() {
    let text = "source raw : Image<Photo,Native> [id]\n\
source gps : GpsTrack [id]\n\
source imu : ImuTrace [id]\n\
source metadata : CaptureMetadata [id]\n\
operation import_photo(image: Image<Photo,$Space>, gps: GpsTrack, imu: ImuTrace, metadata: CaptureMetadata) -> Image<Photo,$Space>\n\
result = import_photo(raw, gps, imu, metadata)\n\
sources:\n\
  raw[id=x]\n\
  gps[id=x]\n\
  imu[id=x]\n\
  metadata[id=x]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    assert_eq!(dag.jobs[0].output.artifact_type, product("Image<Photo,Native>"));
}

#[test]
fn multiple_variables_infer_independently() {
    let mut substitutions = Substitutions::default();
    substitutions
        .unify(&signature("Signal<A>"), &product("Signal<Native>"))
        .unwrap();
    substitutions
        .unify(&signature("Calibration<B>"), &product("Calibration<Standard>"))
        .unwrap();
    assert_eq!(
        substitutions.substitute(&signature("Affine<A,B>")),
        product("Affine<Native,Standard>")
    );
}

#[test]
fn conflicting_variable_reports_previous_and_required_types() {
    let mut substitutions = Substitutions::default();
    substitutions
        .unify(&signature("A<X>"), &product("A<Native>"))
        .unwrap();
    assert!(matches!(
        substitutions.unify(&signature("B<X>"), &product("B<Standard>")),
        Err(TypeUnifyError::VariableConflict { variable, previous, required })
            if variable == "X" && previous == product("Native") && required == product("Standard")
    ));
}

#[test]
fn nested_types_unify_recursively() {
    let mut substitutions = Substitutions::default();
    substitutions
        .unify(
            &signature("Wrapper<Image<S>>"),
            &product("Wrapper<Image<Native>>"),
        )
        .unwrap();
    assert_eq!(substitutions.0["S"], product("Native"));
}

#[test]
fn explicit_long_type_variables_are_distinct_from_named_types() {
    let signature_type = signature("Stream<Frame<$Kind,$SourceSpace>,$Processing>");
    assert_eq!(
        signature_type.to_string(),
        "Stream<Frame<$Kind,$SourceSpace>,$Processing>"
    );
    let mut substitutions = Substitutions::default();
    substitutions
        .unify(
            &signature_type,
            &product("Stream<Frame<Camera,Sensor>,Raw>"),
        )
        .unwrap();
    assert_eq!(substitutions.0["Kind"], product("Camera"));
    assert_eq!(substitutions.0["SourceSpace"], product("Sensor"));
    assert_eq!(substitutions.0["Processing"], product("Raw"));
    assert_eq!(signature("Frame<Kind>"), product("Frame<Kind>"));
}

#[test]
fn explicit_type_variables_are_rejected_in_product_types() {
    assert!(parse_type_expr("Frame<$Kind>", false).is_err());
    assert!(parse_type_expr("Frame<$Kind<Native>>", true).is_err());
}

#[test]
fn later_known_input_refines_an_earlier_partial_variable_binding() {
    let mut substitutions = Substitutions::default();
    substitutions
        .unify(&signature("A"), &product("Frame<Unknown>"))
        .unwrap();
    substitutions
        .unify(&signature("A"), &product("Frame<Foo>"))
        .unwrap();
    assert_eq!(
        substitutions.substitute(&signature("A")),
        product("Frame<Foo>")
    );
    assert!(matches!(
        substitutions.unify(&signature("A"), &product("Frame<Bar>")),
        Err(TypeUnifyError::VariableConflict { .. })
    ));
}

#[test]
fn partial_type_cannot_hide_a_downstream_known_conflict() {
    let text = "products:\n  partly : Frame<Unknown> [id]\n  known : Frame<Foo> [id]\n  merged [id]\n  final : Frame<Bar> [id]\noperations:\n  merge(A, A) -> A\n  sink(Frame<Bar>) -> Frame<Bar>\npipeline:\n  merged = merge(partly, known)\n  final = sink(merged)\n";
    let pipeline = parse_pipeline(text).unwrap();
    let empty = parse_source_inventory("sources:\n").unwrap();
    assert!(matches!(
        resolve(&pipeline, &empty),
        Err(ResolveError::TypeMismatch { operation, .. }) if operation == "sink"
    ));
}

#[test]
fn unknown_is_indeterminate_without_binding_a_variable() {
    let mut substitutions = Substitutions::default();
    assert_eq!(
        substitutions.unify(&signature("Signal<S>"), &TypeExpr::Unknown),
        Ok(Compatibility::Unknown)
    );
    assert!(substitutions.0.is_empty());
    assert_eq!(
        substitutions.substitute(&signature("FilteredSignal<S>")),
        signature("FilteredSignal<S>")
    );
}

#[test]
fn unresolved_output_variables_become_unknown_at_job_boundary() {
    let pipeline = parse_pipeline(
        "products:\n  raw : Unknown [site]\n  result : Unknown [site]\noperations:\n  transform(A<S>) -> B<S>\npipeline:\n  result = transform(raw)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  raw[site=A]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs[0].output.artifact_type, product("B<Unknown>"));
}

#[test]
fn generic_pipeline_infers_output_type_without_pipeline_annotations() {
    let pipeline = parse_pipeline(
        "products:\n  signal : Signal<Native> [site]\n  denoised : Unknown [site]\noperations:\n  denoise(Signal<S>) -> FilteredSignal<S>\npipeline:\n  denoised = denoise(signal)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  signal[site=A]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(
        dag.jobs[0].output.artifact_type,
        product("FilteredSignal<Native>")
    );
}

#[test]
fn chained_generic_operations_propagate_concrete_type() {
    let pipeline = parse_pipeline(
        "products:\n  raw : A<Native> [site]\n  middle : Unknown [site]\n  final : Unknown [site]\noperations:\n  f(A<X>) -> B<X>\n  g(B<Y>) -> C<Y>\npipeline:\n  middle = f(raw)\n  final = g(middle)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  raw[site=A]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs[0].output.artifact_type, product("B<Native>"));
    assert_eq!(dag.jobs[1].output.artifact_type, product("C<Native>"));
}

#[test]
fn operation_type_variables_do_not_leak_between_invocations() {
    let pipeline = parse_pipeline(
        "products:\n  a : A<Native> [site]\n  b : A<Standard> [site]\n  out_a : Unknown [site]\n  out_b : Unknown [site]\noperations:\n  convert(A<X>) -> B<X>\npipeline:\n  out_a = convert(a)\n  out_b = convert(b)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  a[site=01]\n  b[site=01]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs[0].output.artifact_type, product("B<Native>"));
    assert_eq!(dag.jobs[1].output.artifact_type, product("B<Standard>"));
}

#[test]
fn field_survey_reuses_image_operations_across_kinds_and_spaces() {
    let pipeline = parse_pipeline(include_str!("../examples/commands/field_survey.spit")).unwrap();
    let inventory =
        parse_source_inventory(include_str!("../examples/commands/field_survey.sources")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    let output_type = |name: &str| {
        dag.jobs
            .iter()
            .find(|job| job.output.product == name)
            .unwrap()
            .output
            .artifact_type
            .clone()
    };

    assert_eq!(output_type("dark_frame"), product("Image<Dark,Captured>"));
    assert_eq!(output_type("visit_dark"), product("Image<Dark,Orthorectified>"));
    assert_eq!(output_type("map_photo"), product("Image<Map,Orthorectified>"));
    assert_eq!(
        output_type("regions_photo"),
        product("Image<Classes,Orthorectified>")
    );
    assert_eq!(
        pipeline
            .operations
            .iter()
            .filter(|operation| operation.name == "resample")
            .count(),
        1
    );
}

#[test]
fn analytics_join_key_variables_reject_mismatched_relations() {
    let valid = parse_document(include_str!("../examples/analytics/analytics.spit")).unwrap();
    assert_eq!(resolve(&valid.0, &valid.1.unwrap()).unwrap().jobs.len(), 34);

    let invalid = parse_document(include_str!(
        "../examples/analytics/analytics_bad_join.spit"
    ))
    .unwrap();
    assert!(matches!(
        resolve(&invalid.0, &invalid.1.unwrap()),
        Err(ResolveError::TypeVariableConflict { .. })
    ));
}

#[test]
fn conflicting_port_bindings_are_a_structured_resolver_error() {
    let pipeline = parse_pipeline(
        "products:\n  a : A<Native> [site]\n  b : B<Standard> [site]\n  c : Unknown [site]\noperations:\n  op(A<X>, B<X>) -> C<X>\npipeline:\n  c = op(a, b)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  a[site=01]\n  b[site=01]\n").unwrap();
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::TypeVariableConflict { operation, port, variable, .. })
            if operation == "op" && port == "input2" && variable == "X"
    ));
}

#[test]
fn declared_output_type_cannot_contradict_inferred_type() {
    let pipeline = parse_pipeline(
        "products:\n  raw : A<Native> [site]\n  result : B<Standard> [site]\noperations:\n  f(A<X>) -> B<X>\npipeline:\n  result = f(raw)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  raw[site=01]\n").unwrap();
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::TypeVariableConflict { port, variable, .. })
            if port == "output" && variable == "X"
    ));
}
