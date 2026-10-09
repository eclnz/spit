mod support;

use spit::{pipeline_hovers, Hover, HoverKind};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use support::Tree;

fn hovers(text: &str) -> Vec<Hover> {
    pipeline_hovers(text, Path::new("unsaved.spit"))
}

fn at(hovers: &[Hover], line: usize, name: &str) -> Hover {
    hovers
        .iter()
        .find(|hover| hover.line == line && hover.name == name)
        .unwrap()
        .clone()
}

#[test]
fn calls_specialise_generics_and_products_across_a_chain() {
    let text = "source raw : Frame<Native> [sample]\n\
                source reference : Reference<Target> [sample]\n\
                operation clean(input: Frame<S>) -> CleanFrame<S>\n\
                cleaned = clean(raw)\n\
                operation align(moving: CleanFrame<A>, reference: Reference<B>) -> Transform<A,B>\n\
                transform = align(cleaned, reference)\n";
    let all = hovers(text);
    assert_eq!(
        at(&all, 4, "cleaned").signature,
        "cleaned: CleanFrame<Native> [sample]"
    );
    let align = at(&all, 6, "align");
    assert!(align.signature.contains("Transform<$A,$B>"));
    assert!(align
        .details
        .iter()
        .any(|detail| detail.contains("A = Native, B = Target")));
    assert!(align
        .details
        .iter()
        .any(|detail| detail.contains("expects CleanFrame<Native>")));
    assert_eq!(
        at(&all, 6, "transform").signature,
        "transform: Transform<Native,Target> [sample]"
    );
    assert!(at(&all, 4, "raw")
        .details
        .iter()
        .any(|detail| detail.contains("Supplies port input")));
    assert!(at(&all, 1, "raw")
        .details
        .iter()
        .any(|detail| detail.contains("Used by: cleaned = clean")));
}

#[test]
fn bindings_are_local_and_multi_outputs_have_their_own_types() {
    let all = hovers(
        "source native : Frame<Native> [id]\n\
                      source target : Frame<Target> [id]\n\
                      operation split(input: Frame<S>) -> (image: Frame<S>, report: Report<S>)\n\
                      first, first_report = split(native)\n\
                      second, second_report = split(target)\n",
    );
    assert!(at(&all, 4, "split")
        .details
        .contains(&"Type bindings: S = Native".to_owned()));
    assert!(at(&all, 5, "split")
        .details
        .contains(&"Type bindings: S = Target".to_owned()));
    assert_eq!(
        at(&all, 4, "first_report").signature,
        "first_report: Report<Native> [id]"
    );
    assert_eq!(
        at(&all, 5, "second").signature,
        "second: Frame<Target> [id]"
    );
}

#[test]
fn aggregate_hovers_show_inference_and_output_dimensions() {
    let all = hovers("source raw : Frame<Native> [id, run]\noperation merge(items: many Frame<S> @ min(2)) -> Frame<S>\nmerged : Frame<Native> [id] = merge(raw @ vary(run))\n");
    assert_eq!(
        at(&all, 3, "merged").signature,
        "merged: Frame<Native> [id]"
    );
    let call = at(&all, 3, "merge");
    assert!(call.signature.contains("many Frame<$S>"));
    assert!(
        call.signature
            .contains("(items: many Frame<$S> @ min(2)) -> "),
        "{}",
        call.signature
    );
    assert!(call
        .details
        .iter()
        .any(|detail| detail.contains("output → merged: Frame<Native> [id]")));
    let input = at(&all, 3, "raw");
    assert_eq!(
        input.end_column - input.column,
        3,
        "selectors are outside the product hover"
    );
    assert!(!all.iter().any(|hover| hover.name == "run"));
}

#[test]
fn product_paths_report_explicit_nearest_stage_pipeline_and_builtin_rules() {
    let text = "source raw : Lines [id]\npath raw: in/{id}.txt\n\
                path: results/{@product}/{@entities}.txt\noperation copy(input: Lines) -> Lines\n\
                global = copy(raw)\n\
                stage outer:\n    path: outer/{@product}/{@entities}.txt\n    stage inner:\n        inherited = copy(raw)\n        explicit = copy(raw)\n\
                path explicit: special/{id}.txt\n";
    let all = hovers(text);
    assert!(at(&all, 5, "global")
        .details
        .iter()
        .any(|detail| detail.contains("pipeline default")));
    let inherited = at(&all, 9, "inherited");
    assert!(inherited.details.contains(&"Stage: outer/inner".to_owned()));
    assert!(inherited
        .details
        .iter()
        .any(|detail| detail.contains("inherited from stage outer")));
    assert!(at(&all, 10, "explicit")
        .details
        .iter()
        .any(|detail| detail.contains("special/{id}.txt (explicit product rule)")));
    assert_eq!(at(&all, 11, "explicit").kind, HoverKind::Product);
    let defaults = hovers("source raw [id]\noperation copy(input)\nout = copy(raw)\n");
    assert!(at(&defaults, 3, "out")
        .details
        .iter()
        .any(|detail| detail.contains("out/out/{@entities} (built-in output default)")));
    assert!(at(&defaults, 1, "raw")
        .details
        .iter()
        .any(|detail| detail.contains("No pipeline path rule")));
}

#[test]
fn imported_symbols_and_command_references_have_precise_hovers() {
    let tree = Tree::new("hovers-imports", &[]);
    tree.write("lib.spit", "source raw : Frame<Native> [id]\npath raw: in/{id}.txt\noperation clean(input: Frame<S>) -> Frame<S>\ncommand clean: cp {input} {@output}\n");
    let text = "use lib.spit as prep\nout = prep::clean(prep::raw)\noperation copy(input: Frame<S>) -> Frame<S>\ncommand copy: cp {input} {@output}\nverify copy: test -f {input}\n";
    let all = pipeline_hovers(text, &tree.path().join("unsaved.spit"));
    assert!(
        !all.iter().any(|hover| hover.line == 1),
        "do not hover an entire import line as an arbitrary symbol"
    );
    assert_eq!(
        at(&all, 2, "prep::raw").signature,
        "prep::raw: Frame<Native> [id]"
    );
    assert!(at(&all, 2, "prep::clean")
        .details
        .iter()
        .any(|detail| detail == "Command: cp {input} {@output}"));
    assert_eq!(
        at(&all, 2, "prep::clean").end_column - at(&all, 2, "prep::clean").column,
        11
    );
    assert_eq!(at(&all, 4, "copy").kind, HoverKind::Operation);
    assert!(at(&all, 5, "copy")
        .details
        .iter()
        .any(|detail| detail == "Verify: test -f {input}"));
}

#[test]
fn broken_steps_do_not_claim_inferred_types_or_hide_independent_symbols() {
    let all = hovers(
        "source raw : Frame<Native> [id]\noperation copy(input: Frame<S>) -> Frame<S>\n\
                      good = copy(raw)\nbroken = copy(missing)\nbad syntax\nlater = copy(good)\n",
    );
    assert_eq!(at(&all, 3, "good").signature, "good: Frame<Native> [id]");
    assert_eq!(at(&all, 6, "later").signature, "later: Frame<Native> [id]");
    assert_eq!(at(&all, 4, "broken").signature, "broken: Unknown []");
    assert!(at(&all, 4, "copy")
        .details
        .iter()
        .any(|detail| detail.contains("could not be checked")));
    assert!(!all
        .iter()
        .any(|hover| hover.name == "missing" || hover.line == 5));
}

#[test]
fn hover_ranges_use_utf16_and_exclude_comments_and_literal_text() {
    let text = "\u{feff}source raw [id]\noperation copy(first, second)\nout = copy(raw @ where(id=😀), raw) # raw copy\ncommand copy: echo raw {@output}\n";
    let all = hovers(text);
    assert_eq!(at(&all, 1, "raw").column, 9);
    assert_eq!(at(&all, 3, "copy").column, 7);
    let inputs: Vec<_> = all
        .iter()
        .filter(|hover| hover.line == 3 && hover.name == "raw")
        .collect();
    assert_eq!(inputs.len(), 2, "the comment is not a reference");
    assert_eq!(inputs[0].column, 12);
    assert_eq!(
        inputs[1].column, 32,
        "astral selector values use two UTF-16 units"
    );
    assert!(!all
        .iter()
        .any(|hover| hover.line == 4 && hover.name == "raw"));
}

#[test]
fn cli_editor_analysis_is_opt_in_and_preserves_error_diagnostics() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", "unsaved.spit", "--json", "--stdin", "--hovers"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"source raw [id]\noperation copy(input)\ngood = copy(raw)\nbad syntax\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("\"severity\":\"error\""));
    assert!(json.contains("\"hovers\":["));
    assert!(json.contains("\"signature\":\"good: Unknown [id]\""));
    let invalid = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", "unsaved.spit", "--hovers"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8(invalid.stderr)
        .unwrap()
        .contains("--hovers requires --json"));
}

#[test]
fn hovers_keep_resolved_path_hints_and_extensions() {
    let text = "path: out/[{@labels}_]{@product}\next: .txt\nsource raw [sub]\npath raw: in/{sub}.txt\noperation copy(input)\nresult = copy(raw)\n";
    let all = hovers(text);
    assert!(at(&all, 6, "result")
        .details
        .iter()
        .any(|detail| detail.contains("out/sub-{sub}_result.txt (pipeline default)")));

    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", "unsaved.spit", "--json", "--stdin", "--hovers"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(
        json.contains("\"path\":\"out/sub-{sub}_result.txt\""),
        "{json}"
    );
    assert!(json.contains("\"hovers\":["), "{json}");
}

#[test]
fn path_hints_keep_literal_braces_and_brackets_doubled() {
    // `{{lit}}` and `[[x]]` are literal text: the hint writes them as the rule
    // does, so they are not read as a placeholder and a group.
    let text = "path: out/{{lit}}/[[x]]/{@product}/{sub}.txt\nsource raw [sub]\npath raw: in/{sub}.txt\noperation copy(input)\nresult = copy(raw)\n";
    let all = hovers(text);
    assert!(at(&all, 5, "result")
        .details
        .iter()
        .any(|detail| detail.contains("out/{{lit}}/[[x]]/result/{sub}.txt (pipeline default)")));
}

#[test]
fn beside_outputs_keep_their_path_and_signature_in_hovers() {
    let all = hovers(
        "source raw [id]\npath raw: in/{id}.txt\npath: out/{@product}/{@entities}\noperation convert(input) -> (image: Image .nii.gz, meta: Json .json beside image)\nimage, meta = convert(raw)\n",
    );
    assert!(at(&all, 4, "convert")
        .signature
        .contains("meta: Json .json beside image"));
    assert!(at(&all, 5, "meta")
        .details
        .iter()
        .any(|detail| detail.contains("out/image/{@entities}.json (beside image)")));
}

#[test]
fn a_source_shows_its_extension_and_no_stage_default() {
    let text = "path: {@stage}/{@product}/{@entities}\n\
                source events : Events .tsv [sub]\n\
                stage prep:\n    operation f(x)\n    y = f(events)\n";
    let events = at(&hovers(text), 2, "events");
    assert_eq!(events.signature, "events: Events .tsv [sub]");
    // A default that needs `{@stage}` is no source's rule.
    assert_eq!(
        events.details.last().unwrap(),
        "No pipeline path rule; a recipe or inventory must supply the source path."
    );
}

const REGISTRATION: &str = "\
source raw_t1: MRI<T1w,Native,Original> [sub]
operation parcellate(image: MRI<$I,$Space,$Grid>) -> (parc: MRI<Parc,$Space,SynthGrid<$Grid>>, resampled: MRI<$I,$Space,SynthGrid<$Grid>>)
operation brainmask(image: MRI<Parc,$Space,$Grid>) -> MRI<Mask,$Space,$Grid>
operation prep_registration(image: MRI<$I,$Space,$Grid>) -> (parc: MRI<Parc,$Space,SynthGrid<$Grid>>, resampled: MRI<$I,$Space,SynthGrid<$Grid>>, mask: MRI<Mask,$Space,SynthGrid<$Grid>>):
    parc, resampled = parcellate(image)
    mask = brainmask(parc)
t1_parc, t1_resampled, t1_mask = prep_registration(raw_t1)
command parcellate: tool {image} {parc} {resampled}
command brainmask: tool {image} {@output}
";

#[test]
fn composite_hovers_show_typed_ports_and_one_body_in_the_right_context() {
    let all = hovers(REGISTRATION);
    let declaration = at(&all, 4, "prep_registration");
    assert!(declaration.signature.contains("\n    image: MRI<"));
    assert!(declaration.signature.contains(") -> (\n    parc: MRI<"));
    assert!(declaration.signature.contains(",\n    resampled: MRI<"));
    assert_eq!(declaration.details, ["Carried out by the steps in its body: parc, resampled = parcellate(image)\nmask = brainmask(parc)"]);
    let call = at(&all, 7, "prep_registration");
    assert!(call
        .details
        .iter()
        .all(|detail| !detail.starts_with("Carried out")));
    assert!(call
        .details
        .iter()
        .any(|detail| detail == "image ← raw_t1: MRI<T1w,Native,Original> [sub]"));
    assert!(
        call.details
            .iter()
            .any(|detail| detail
                .contains("mask → t1_mask: MRI<Mask,Native,SynthGrid<Original>> [sub]")),
        "{:?}",
        call.details
    );
    assert_eq!(
        call.details
            .iter()
            .filter(|detail| detail.starts_with("This call expands to:"))
            .count(),
        1
    );
    assert!(call.details.iter().any(|detail| detail == "This call expands to: t1_parc, t1_resampled = parcellate(raw_t1)\nt1_mask = brainmask(t1_parc)"));
}

#[test]
fn primitive_calls_prioritise_bindings_and_keep_execution_information() {
    let all = hovers("source raw: Text [id]\noperation copy(input: Text) -> Text\ncommand copy: cp {input} {@output}\nverify copy: test -f {input}\nout = copy(raw)\n");
    let declaration = at(&all, 2, "copy");
    assert!(declaration.details[0].starts_with("Single-artifact"));
    let call = at(&all, 5, "copy");
    assert_eq!(call.signature, "operation copy(input: Text) -> Text");
    assert_eq!(call.details[0], "This call:");
    assert!(call
        .details
        .contains(&"Command: cp {input} {@output}".to_owned()));
    assert!(call.details.contains(&"Verify: test -f {input}".to_owned()));
    assert!(call
        .details
        .iter()
        .all(|detail| !detail.starts_with("Single-artifact")));
}

#[test]
fn product_hovers_keep_inference_failures_and_bound_large_reader_lists() {
    let mut text = "source raw: Text [id]\noperation copy(input: Text) -> Text\n".to_owned();
    for index in 0..12 {
        text.push_str(&format!("out{index} = copy(raw)\n"));
    }
    text.push_str("broken = copy(missing)\n");
    let all = hovers(&text);
    let source = at(&all, 1, "raw");
    assert!(source
        .details
        .contains(&"4 other steps also use this product.".to_owned()));
    let readers = source
        .details
        .iter()
        .find(|detail| detail.starts_with("Used by:"))
        .unwrap();
    assert!(readers.contains("out7 = copy"));
    assert!(!readers.contains("out8 = copy"));
    assert!(at(&all, 3, "out0")
        .details
        .iter()
        .all(|detail| !detail.starts_with("Declared type:")));
    assert!(at(&all, 15, "broken")
        .details
        .iter()
        .any(|detail| detail.starts_with("Inference unavailable")));
}

#[test]
fn call_hovers_explain_the_selectors_that_change_matching() {
    let all = hovers("source raw: Text [sub, run]\nsource reference: Text [run]\noperation copy(input: Text, driver: Text) -> Text\npinned = copy(raw @ where(run=1) @ same(sub), raw)\nbroadcast = copy(raw @ each(sub), reference)\n");
    let pinned = at(&all, 4, "copy");
    assert!(pinned.details.contains(&"Pins input to run=1.".to_owned()));
    assert!(pinned.details.contains(&"Matches input on sub.".to_owned()));
    assert!(at(&all, 5, "copy")
        .details
        .contains(&"Broadcasts input across sub.".to_owned()));
    let aggregate = hovers("source raw: Text [sub, run]\noperation merge(items: many Text) -> Text\nmerged = merge(raw @ vary(run))\n");
    let call = at(&aggregate, 3, "merge");
    assert!(call.details[1].contains("items ←"));
    assert!(call.details[2].contains("output →"));
    assert_eq!(call.details[3], "Collects items across run.");
}

#[test]
fn a_broken_composite_retains_its_contract_without_claiming_inferred_results() {
    let text = REGISTRATION.replace("MRI<T1w,Native,Original>", "Text");
    let all = hovers(&text);
    let call = at(&all, 7, "prep_registration");
    assert!(call
        .details
        .iter()
        .any(|detail| detail.contains("could not be fully checked")));
    assert!(call
        .details
        .iter()
        .all(|detail| !detail.contains("MRI<Mask,Native,SynthGrid<Original>>")));
    assert!(call
        .details
        .iter()
        .any(|detail| detail.starts_with("This call expands to:")));
    assert_eq!(at(&all, 4, "prep_registration").details.len(), 1);
}
