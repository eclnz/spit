use std::path::Path;

use spit::{diagnose, parse_pipeline, pipeline_hovers};

#[test]
fn multiline_lists_match_single_line_signatures() {
    let single = "operation prep(image: MRI<$I,$Space,$Grid>, reference: MRI<$I,$Space,$Grid>) -> (parc: MRI<Parc,$Space,SynthGrid<$Grid>>, mask: MRI<Mask,$Space,SynthGrid<$Grid>>)\n";
    let multiline = "operation prep(\n    image: MRI<$I,$Space,$Grid>, # input\n    reference: MRI<$I,$Space,$Grid>\n) -> (\n    parc: MRI<Parc,$Space,SynthGrid<$Grid>>,\n    # output\n    mask: MRI<Mask,$Space,SynthGrid<$Grid>>\n)\n";
    assert_eq!(
        format!("{:?}", parse_pipeline(single).unwrap().operations),
        format!("{:?}", parse_pipeline(multiline).unwrap().operations)
    );
    for text in [
        "operation prep(\n    image: Image,\n    mask: Mask\n) -> Result\n",
        "operation prep(image: Image) -> (\n    image: Image,\n    mask: Mask\n)\n",
        "operation prep(\n    image: Image @ check(nonempty),\n    mask: Mask\n) -> (\n    result: Image @ check(nonempty),\n    mask: Mask\n)\ncheck nonempty: test -s {@file}\n",
    ] {
        parse_pipeline(text).unwrap();
    }
}

#[test]
fn stages_and_composite_bodies_start_after_the_complete_signature() {
    let text = "operation clean(x: Image) -> Image\nstage prep:\n    operation wrap(\n        x: Image\n    ) -> (\n        result: Image\n    ):\n        result = clean(x)\n    operation next(\n        x: Image\n    ) -> Image\noperation last(x: Image) -> Image\n";
    let pipeline = parse_pipeline(text).unwrap();
    assert_eq!(pipeline.operations.len(), 4);
    assert_eq!(pipeline.operations[1].steps.len(), 1);
    assert!(pipeline.operations[2].steps.is_empty());
    let hovers = pipeline_hovers(text, Path::new("unsaved.spit"));
    for (name, line, start) in [("wrap", 3, 14), ("next", 9, 14), ("last", 12, 10)] {
        let hover = hovers
            .iter()
            .find(|hover| hover.name == name && hover.line == line)
            .unwrap();
        assert_eq!(hover.column, start + 1);
    }
}

#[test]
fn malformed_port_points_at_its_physical_line() {
    let text = "operation broken(\n    first: Image,\n    second: Image<Bad]>\n) -> Image\noperation later(x: Image) -> Image\n";
    let error = parse_pipeline(text).unwrap_err();
    assert_eq!(error.line(), 3);
    assert_eq!(error.location.columns, Some(21..22));
    let hovers = pipeline_hovers(text, Path::new("unsaved.spit"));
    assert!(hovers
        .iter()
        .any(|hover| hover.name == "later" && hover.line == 5));
    let diagnosis = diagnose(text, None);
    assert_eq!(diagnosis.len(), 1);
}

#[test]
fn unclosed_signature_does_not_consume_later_declarations() {
    for text in [
        "operation broken(\n    x: Image\noperation later(x: Image) -> Image\n",
        "operation broken(x: Image) -> (\n    result: Image\noperation later(x: Image) -> Image\n",
    ] {
        assert!(parse_pipeline(text).is_err());
        assert!(pipeline_hovers(text, Path::new("unsaved.spit"))
            .iter()
            .any(|hover| hover.name == "later" && hover.line == 3));
    }
}
