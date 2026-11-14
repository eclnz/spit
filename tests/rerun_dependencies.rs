//! Explicit tool identities and inputs stored beside a pipeline.

mod support;
use support::{spit, text, Tree};

#[test]
fn local_sources_are_inputs_under_the_pipeline_folder() {
    let tree = Tree::new("local-source", &["data/in.txt", "scripts/plot.py"]);
    let main = tree.write("main.spit", "root data\nsource raw: Text\npath raw: in.txt\nsource plotter: Script .py here\npath plotter: scripts/plot.py\noperation plot(raw: Text, script: Script) -> Text .txt\ncommand plot: python3 {script} {raw} {@output}\nresult = plot(raw, plotter)\npath result: out.txt\n");
    let output = spit(&["dag", main.to_str().unwrap(), "--json"]);
    let json = text(&output.stdout);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        json.contains("\"path\":\"scripts/plot.py\",\"under\":\"pipeline_folder\""),
        "{json}"
    );
    assert!(json.contains("{\"path\":\"scripts/plot.py\"}"), "{json}");
}

#[test]
fn imported_local_sources_use_the_library_folder() {
    let tree = Tree::new("import-local-source", &["data/in.txt", "lib/plot.py"]);
    tree.write(
        "lib/plot.spit",
        "source plotter: Script .py here\npath plotter: plot.py\n",
    );
    let main = tree.write("main.spit", "use lib/plot.spit\nroot data\noperation plot(script: Script) -> Text .txt\ncommand plot: python3 {script} {@output}\nresult = plot(plotter)\npath result: out.txt\n");
    let output = spit(&["dag", main.to_str().unwrap(), "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("\"path\":\"lib/plot.py\",\"under\":\"pipeline_folder\""));
}

#[test]
fn recipe_tools_are_embedded_and_narrowed_per_operation() {
    let tree = Tree::new("recipe-tools", &["data/in.txt"]);
    tree.write("main.spit", "source raw: Text\npath raw: in.txt\ntool compiler\ntool extra\noperation copy(raw: Text) -> Text .txt\nuses copy: compiler\ncommand copy: cp {raw} {@output}\nresult = copy(raw)\npath result: out.txt\n");
    tree.write(
        "host.spitenv",
        "tools:\n    compiler: build-1\n    extra: probe printf extra\n",
    );
    let recipe = tree.write(
        "run.spitin",
        "pipeline main.spit\nroot data\ntools from host.spitenv\n",
    );
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    let json = text(&output.stdout);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        json.contains("\"name\":\"compiler\",\"identity\":\"build-1\",\"probe\":null"),
        "{json}"
    );
    assert!(json.contains("\"tools\":[\"compiler\"]"), "{json}");
    let output = spit(&[
        "check",
        tree.write("host.spitenv", "tools:\n    compiler: build-1\n")
            .to_str()
            .unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
}
