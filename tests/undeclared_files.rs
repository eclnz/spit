//! The note that names a file a command or check runs without declaring it,
//! so that editing the file reruns nothing.

mod support;

use support::{spit, Tree};

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A pipeline whose `clean` runs a script by a path under the root, one
/// beside the pipeline by `{@here}`, and one that doesn't exist.
fn recipe(tree: &Tree) -> String {
    tree.write(
        "pipeline.spit",
        "source sales : Sales [store]\npath sales: data/sales/{store}.csv\n\
         check parsed: python3 tools/parse.py {@path}\n\
         operation clean(sales: Sales) -> Sales @check(parsed)\n\
         command clean: python3 tools/clean.py {@here}/lib/rules.py tools/missing.py /usr/bin/env {sales} {@output}\n\
         path cleaned: out/{store}.csv\ncleaned = clean(sales)\n",
    );
    let recipe = tree.write("dataset.spitin", "pipeline pipeline.spit\nroot data\n");
    recipe.to_str().unwrap().to_owned()
}

#[test]
fn a_file_a_command_runs_without_declaring_it_is_named() {
    let tree = Tree::new(
        "undeclared-files",
        &[
            "data/sales/s01.csv",
            "data/tools/clean.py",
            "data/tools/parse.py",
            "lib/rules.py",
        ],
    );
    let recipe = recipe(&tree);
    for command in ["inputs", "dag"] {
        let output = spit(&[command, &recipe]);
        assert!(output.status.success(), "{}", stderr(&output));
        let stderr = stderr(&output);
        for named in [
            "check `parsed` runs `tools/parse.py`",
            "operation `clean` runs `tools/clean.py`",
            "operation `clean` runs `{@here}/lib/rules.py`",
        ] {
            assert_eq!(
                stderr.matches(named).count(),
                1,
                "{command}: {named}\n{stderr}"
            );
        }
        assert!(stderr.contains("so editing the file reruns nothing; to rerun on a change, keep it under the dataset root and pass it to `clean` as a source\n"));
        assert!(!stderr.contains("missing.py"), "{stderr}");
        assert!(!stderr.contains("/usr/bin/env"), "{stderr}");
    }
}
