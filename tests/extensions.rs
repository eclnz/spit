//! An operation may declare the extension its tool gives each output's file,
//! and `ext:` the extension a default path is completed with, so a path rule
//! need not be copied only to change its extension.

mod support;

use std::fs;

use spit::{diagnose, parse_pipeline, parse_pipeline_at, Pipeline};
use support::{spit, text, Tree};

/// The path `product` is written to, as its rules and extensions give it.
fn path(pipeline: &Pipeline, product: &str) -> String {
    pipeline.path_template_for(product).unwrap().to_string()
}

/// Every error message `diagnose` finds in `text`, with its line.
fn errors(text: &str) -> Vec<(Option<usize>, String)> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| (diagnostic.line, diagnostic.message))
        .collect()
}

const STEPS: &str = "\
source raw : Image [id]
path raw: in/{id}.raw
operation copy(input: Image) -> Image
operation align(input: Image) -> Transform .mat
operation fit(input: Image) -> (weights: Weights .npz, quality: Metrics .tar.gz)
operation untyped(input) -> .txt
copied = copy(raw)
matrix = align(raw)
weights, quality = fit(raw)
note = untyped(raw)
";

#[test]
fn an_operation_completes_the_default_path_with_its_outputs_extensions() {
    let pipeline =
        parse_pipeline(&format!("path: out/{{@product}}/{{@entities}}\n{STEPS}")).unwrap();
    assert_eq!(path(&pipeline, "copied"), "out/{@product}/{@entities}");
    assert_eq!(path(&pipeline, "matrix"), "out/{@product}/{@entities}.mat");
    assert_eq!(path(&pipeline, "weights"), "out/{@product}/{@entities}.npz");
    assert_eq!(
        path(&pipeline, "quality"),
        "out/{@product}/{@entities}.tar.gz"
    );
    assert_eq!(path(&pipeline, "note"), "out/{@product}/{@entities}.txt");
    // A source's own rule is untouched.
    assert_eq!(path(&pipeline, "raw"), "in/{id}.raw");
}

#[test]
fn ext_completes_the_default_for_operations_that_declare_none() {
    let pipeline = parse_pipeline(&format!(
        "path: out/{{@product}}/{{@entities}}\next: .img\n{STEPS}"
    ))
    .unwrap();
    assert_eq!(path(&pipeline, "copied"), "out/{@product}/{@entities}.img");
    // The operation's own extension wins.
    assert_eq!(path(&pipeline, "matrix"), "out/{@product}/{@entities}.mat");
}

#[test]
fn a_stage_ext_applies_to_its_steps_and_nested_stages() {
    let pipeline = parse_pipeline(
        "path: out/{@product}/{@entities}\n\
         ext: .mif\n\
         source raw : Image [id]\n\
         path raw: in/{id}.raw\n\
         operation copy(input: Image) -> Image\n\
         operation align(input: Image) -> Transform .mat\n\
         outside = copy(raw)\n\
         stage anatomy:\n\
         \x20   ext: .nii.gz\n\
         \x20   inside = copy(raw)\n\
         \x20   stage deeper:\n\
         \x20       nested = copy(raw)\n\
         \x20       matrix = align(raw)\n",
    )
    .unwrap();
    assert_eq!(path(&pipeline, "outside"), "out/{@product}/{@entities}.mif");
    assert_eq!(
        path(&pipeline, "inside"),
        "out/{@product}/{@entities}.nii.gz"
    );
    assert_eq!(
        path(&pipeline, "nested"),
        "out/{@product}/{@entities}.nii.gz"
    );
    assert_eq!(path(&pipeline, "matrix"), "out/{@product}/{@entities}.mat");
}

#[test]
fn a_products_own_rule_takes_only_its_operations_extension() {
    let pipeline = parse_pipeline(&format!(
        "path: out/{{@product}}/{{@entities}}\next: .img\n{STEPS}\
         path matrix: transforms/{{id}}\n\
         path weights: fits/{{id}}.npz\n\
         path copied: copies/{{id}}\n"
    ))
    .unwrap();
    // Left off, the operation's extension is added; written, it is kept.
    assert_eq!(path(&pipeline, "matrix"), "transforms/{id}.mat");
    assert_eq!(path(&pipeline, "weights"), "fits/{id}.npz");
    // `ext:` completes only default rules.
    assert_eq!(path(&pipeline, "copied"), "copies/{id}");
}

#[test]
fn a_source_on_the_default_rule_takes_ext() {
    let pipeline = parse_pipeline(
        "path: data/{@product}/{@entities}\next: .csv\nsource table [id]\noperation copy(input)\ncopied = copy(table)\n",
    )
    .unwrap();
    assert_eq!(path(&pipeline, "table"), "data/{@product}/{@entities}.csv");
}

#[test]
fn bound_paths_carry_the_extension() {
    let tree = Tree::new("extensions-bound", &[]);
    tree.write(
        "pipeline.spit",
        &format!(
            "path: out/{{@product}}/{{@entities}}\next: .img\n{STEPS}\
             command copy: copy {{input}} {{output}}\n\
             command align: align {{input}} {{output}}\n\
             command fit: fit {{input}} {{weights}} {{quality}}\n\
             command untyped: note {{input}} {{output}}\n"
        ),
    );
    tree.write("inputs.spitout", "sources:\n    raw[id=a]\n");
    let pipeline = tree.path().join("pipeline.spit");
    let inputs = tree.path().join("inputs.spitout");
    let output = spit(&[
        "dag",
        pipeline.to_str().unwrap(),
        inputs.to_str().unwrap(),
        "--commands",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let commands = text(&output.stdout);
    for line in [
        "copy in/a.raw out/copied/id=a.img",
        "align in/a.raw out/matrix/id=a.mat",
        "fit in/a.raw out/weights/id=a.npz out/quality/id=a.tar.gz",
        "note in/a.raw out/note/id=a.txt",
    ] {
        assert!(commands.contains(line), "{line} in {commands}");
    }
}

#[test]
fn a_rule_ending_in_another_extension_is_an_error() {
    let found = errors(&format!(
        "path: out/{{@product}}/{{@entities}}\n{STEPS}path matrix: transforms/{{id}}.txt\n"
    ));
    assert_eq!(
        found,
        [(
            Some(12),
            "path `matrix` ends in `.txt`, but operation `align` writes `.mat`; drop the extension or use `.mat`".to_owned()
        )]
    );
}

#[test]
fn a_dot_earlier_in_the_file_name_is_not_part_of_its_extension() {
    let pipeline = format!(
        "path: out/{{@product}}/{{@entities}}\n{STEPS}path matrix: transforms/{{id}}_acq-1.5T.mat\n"
    );
    assert_eq!(errors(&pipeline), []);
    assert_eq!(
        path(&parse_pipeline(&pipeline).unwrap(), "matrix"),
        "transforms/{id}_acq-1.5T.mat"
    );
}

#[test]
fn a_default_rule_ending_in_another_extension_is_one_error() {
    // Two products disagree with the default, which is said once.
    let found = errors(&format!(
        "path: out/{{@product}}/{{@entities}}.img\n{STEPS}also = align(raw)\n"
    ));
    let messages: Vec<_> = found.iter().map(|(_, message)| message.as_str()).collect();
    assert!(messages.contains(
        &"the default path ends in `.img`, but operation `align` writes `.mat`; write the default path without an extension, and give the outputs that use it `ext: .img`"
    ), "{messages:?}");
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.contains("`align`"))
            .count(),
        1,
        "{messages:?}"
    );
    // A default written with the extension `ext:` also sets.
    let found = errors(
        "source raw [id]\npath raw: in/{id}\noperation copy(input)\nstage s:\n    path: out/{@product}/{@entities}.img\n    ext: .txt\n    copied = copy(raw)\n",
    );
    assert!(found.iter().any(|(line, message)| *line == Some(5)
        && message == "stage `s`'s default path ends in `.img`, but stage `s`'s `ext:` sets `.txt`; write the extension once, with `ext:`"), "{found:?}");
    // The same extension written in both agrees.
    assert_eq!(
        errors("path: out/{@product}/{@entities}.img\next: .img\nsource raw [id]\npath raw: in/{id}\noperation copy(input)\ncopied = copy(raw)\n"),
        []
    );
}

#[test]
fn extensions_are_checked_when_parsed() {
    let bad = parse_pipeline("source raw [id]\noperation f(x) -> Image mat.\n").unwrap_err();
    assert!(bad.to_string().contains("is not an extension"), "{bad}");
    let bad = parse_pipeline("ext: img\n").unwrap_err();
    assert!(
        bad.to_string().contains("`img` is not an extension"),
        "{bad}"
    );
    let twice = parse_pipeline("ext: .a\next: .b\n").unwrap_err();
    assert!(twice.to_string().contains("duplicate `ext:`"), "{twice}");
    let staged = parse_pipeline("stage s:\n    ext: .a\n    ext: .b\n").unwrap_err();
    assert!(
        staged
            .to_string()
            .contains("duplicate `ext:` for stage `s`"),
        "{staged}"
    );
    // A step may still name its output `ext`.
    let step = parse_pipeline("source raw [id]\noperation f(x)\next: Image = f(raw)\n").unwrap();
    assert!(step.products.iter().any(|product| product.name == "ext"));
}

#[test]
fn a_recipe_cannot_set_ext() {
    let error = spit::parse_input_spec("pipeline a.spit\next: .img\n").unwrap_err();
    assert!(
        error.to_string().contains("belongs in the .spit pipeline"),
        "{error}"
    );
}

#[test]
fn an_imported_operation_brings_its_extension() {
    let tree = Tree::new("extensions-imports", &[]);
    tree.write(
        "tools.spit",
        "path: in/{@product}/{id}\next: .csv\nsource table [id]\noperation align(input) -> Transform .mat\n",
    );
    let main = tree.write(
        "main.spit",
        "path: out/{@product}/{@entities}\nuse table, align from tools.spit as tools\nmatrix = tools::align(tools::table)\n",
    );
    let pipeline = parse_pipeline_at(&fs::read_to_string(&main).unwrap(), &main).unwrap();
    assert_eq!(path(&pipeline, "matrix"), "out/{@product}/{@entities}.mat");
    // A source keeps the path its own file gives it, extension included.
    assert_eq!(path(&pipeline, "tools::table"), "in/table/{id}.csv");
}

#[test]
fn check_says_where_each_extension_comes_from() {
    let tree = Tree::new("extensions-check", &[]);
    let pipeline = tree.write(
        "pipeline.spit",
        &format!("path: out/{{@product}}/{{@entities}}\next: .img\n{STEPS}path matrix: transforms/{{id}}\n"),
    );
    let output = spit(&["check", pipeline.to_str().unwrap(), "--path-rules"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let rules = text(&output.stdout);
    for line in [
        "  raw (source): explicit in/{id}.raw\n",
        "  copied (output): default out/{@product}/{@entities}.img, `.img` from `ext:`\n",
        "  matrix (output): explicit transforms/{id}.mat, `.mat` from operation `align`\n",
        "  quality (output): default out/{@product}/{@entities}.tar.gz, `.tar.gz` from operation `fit`\n",
    ] {
        assert!(rules.contains(line), "{line} in {rules}");
    }
    // For an editor: each path no line writes in full, on its step's line.
    let output = spit(&["check", pipeline.to_str().unwrap(), "--json"]);
    let json = text(&output.stdout);
    assert!(
        json.contains(
            "{\"product\":\"copied\",\"line\":9,\"path\":\"out/copied/{@entities}.img\"}"
        ),
        "{json}"
    );
    assert!(
        json.contains("{\"product\":\"matrix\",\"line\":10,\"path\":\"transforms/{id}.mat\"}"),
        "{json}"
    );
    assert!(!json.contains("\"product\":\"raw\""), "{json}");
}
