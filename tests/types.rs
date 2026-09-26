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
        substitutions.unify(&product("BOLD"), &product("BOLD")),
        Ok(Compatibility::Compatible)
    );
}

#[test]
fn different_named_types_fail() {
    let mut substitutions = Substitutions::default();
    assert!(matches!(
        substitutions.unify(&product("BOLD"), &product("T1w")),
        Err(TypeUnifyError::Mismatch { .. })
    ));
}

#[test]
fn different_constructors_and_arities_fail() {
    let mut substitutions = Substitutions::default();
    assert!(matches!(
        substitutions.unify(&signature("BOLD<S>"), &product("Image<Native>")),
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
        substitutions.unify(&signature("BOLD<S>"), &product("BOLD<Native>")),
        Ok(Compatibility::Compatible)
    );
    assert_eq!(substitutions.0["S"], product("Native"));
    assert_eq!(
        substitutions.substitute(&signature("DenoisedBOLD<S>")),
        product("DenoisedBOLD<Native>")
    );
}

#[test]
fn generic_import_preserves_dwi_space() {
    let text = "source raw : MRI<DWI,Native> [id]\n\
source bvec : GradientDirections [id]\n\
source bval : GradientAmplitudes [id]\n\
source metadata : AcquisitionMetadata [id]\n\
operation import_dwi(image: MRI<DWI,$Space>, bvec: GradientDirections, bval: GradientAmplitudes, metadata: AcquisitionMetadata) -> MRI<DWI,$Space>\n\
result = import_dwi(raw, bvec, bval, metadata)\n\
sources:\n\
  raw[id=x]\n\
  bvec[id=x]\n\
  bval[id=x]\n\
  metadata[id=x]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    assert_eq!(dag.jobs[0].output.artifact_type, product("MRI<DWI,Native>"));
}

#[test]
fn multiple_variables_infer_independently() {
    let mut substitutions = Substitutions::default();
    substitutions
        .unify(&signature("BOLD<A>"), &product("BOLD<Native>"))
        .unwrap();
    substitutions
        .unify(&signature("T1w<B>"), &product("T1w<MNI>"))
        .unwrap();
    assert_eq!(
        substitutions.substitute(&signature("Affine<A,B>")),
        product("Affine<Native,MNI>")
    );
}

#[test]
fn conflicting_variable_reports_previous_and_required_types() {
    let mut substitutions = Substitutions::default();
    substitutions
        .unify(&signature("A<X>"), &product("A<Native>"))
        .unwrap();
    assert!(matches!(
        substitutions.unify(&signature("B<X>"), &product("B<MNI>")),
        Err(TypeUnifyError::VariableConflict { variable, previous, required })
            if variable == "X" && previous == product("Native") && required == product("MNI")
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
        substitutions.unify(&signature("BOLD<S>"), &TypeExpr::Unknown),
        Ok(Compatibility::Unknown)
    );
    assert!(substitutions.0.is_empty());
    assert_eq!(
        substitutions.substitute(&signature("DenoisedBOLD<S>")),
        signature("DenoisedBOLD<S>")
    );
}

#[test]
fn unresolved_output_variables_become_unknown_at_job_boundary() {
    let pipeline = parse_pipeline(
        "products:\n  raw : Unknown [subject]\n  result : Unknown [subject]\noperations:\n  transform(A<S>) -> B<S>\npipeline:\n  result = transform(raw)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  raw[subject=A]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs[0].output.artifact_type, product("B<Unknown>"));
}

#[test]
fn generic_pipeline_infers_output_type_without_pipeline_annotations() {
    let pipeline = parse_pipeline(
        "products:\n  bold : BOLD<Native> [subject]\n  denoised : Unknown [subject]\noperations:\n  denoise(BOLD<S>) -> DenoisedBOLD<S>\npipeline:\n  denoised = denoise(bold)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  bold[subject=A]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(
        dag.jobs[0].output.artifact_type,
        product("DenoisedBOLD<Native>")
    );
}

#[test]
fn chained_generic_operations_propagate_concrete_type() {
    let pipeline = parse_pipeline(
        "products:\n  raw : A<Native> [subject]\n  middle : Unknown [subject]\n  final : Unknown [subject]\noperations:\n  f(A<X>) -> B<X>\n  g(B<Y>) -> C<Y>\npipeline:\n  middle = f(raw)\n  final = g(middle)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  raw[subject=A]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs[0].output.artifact_type, product("B<Native>"));
    assert_eq!(dag.jobs[1].output.artifact_type, product("C<Native>"));
}

#[test]
fn operation_type_variables_do_not_leak_between_invocations() {
    let pipeline = parse_pipeline(
        "products:\n  a : A<Native> [subject]\n  b : A<MNI> [subject]\n  out_a : Unknown [subject]\n  out_b : Unknown [subject]\noperations:\n  convert(A<X>) -> B<X>\npipeline:\n  out_a = convert(a)\n  out_b = convert(b)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  a[subject=01]\n  b[subject=01]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs[0].output.artifact_type, product("B<Native>"));
    assert_eq!(dag.jobs[1].output.artifact_type, product("B<MNI>"));
}

#[test]
fn act_example_reuses_image_operations_across_kinds_and_spaces() {
    let pipeline = parse_pipeline(include_str!("../examples/commands/mrtrix3_act.spit")).unwrap();
    let inventory =
        parse_source_inventory(include_str!("../examples/commands/mrtrix3_act.sources")).unwrap();
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

    assert_eq!(output_type("forward_b0"), product("MRI<B0,Acquired>"));
    assert_eq!(output_type("session_b0"), product("MRI<B0,Diffusion>"));
    assert_eq!(output_type("t1w_dwi"), product("MRI<T1w,Diffusion>"));
    assert_eq!(
        output_type("nodes_dwi"),
        product("MRI<Parcellation,Diffusion>")
    );
    assert_eq!(
        pipeline
            .operations
            .iter()
            .filter(|operation| operation.name == "mrtransform")
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
        "products:\n  a : A<Native> [subject]\n  b : B<MNI> [subject]\n  c : Unknown [subject]\noperations:\n  op(A<X>, B<X>) -> C<X>\npipeline:\n  c = op(a, b)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  a[subject=01]\n  b[subject=01]\n").unwrap();
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::TypeVariableConflict { operation, port, variable, .. })
            if operation == "op" && port == "input2" && variable == "X"
    ));
}

#[test]
fn declared_output_type_cannot_contradict_inferred_type() {
    let pipeline = parse_pipeline(
        "products:\n  raw : A<Native> [subject]\n  result : B<MNI> [subject]\noperations:\n  f(A<X>) -> B<X>\npipeline:\n  result = f(raw)\n",
    )
    .unwrap();
    let inventory = parse_source_inventory("sources:\n  raw[subject=01]\n").unwrap();
    assert!(matches!(
        resolve(&pipeline, &inventory),
        Err(ResolveError::TypeVariableConflict { port, variable, .. })
            if port == "output" && variable == "X"
    ));
}
