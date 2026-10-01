//! The command line: one command per step, with the files it works on given
//! as arguments.

mod support;

use std::fs;
use std::process::Output;

use support::{spit, Tree};

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn help_lists_each_step_and_each_command_explains_itself() {
    for args in [&[][..], &["help"], &["--help"], &["-h"]] {
        let help = spit(args);
        assert!(help.status.success());
        let text = stdout(&help);
        for command in ["check", "inputs", "dag", "artifacts"] {
            assert!(text.contains(&format!("  {command} ")), "{text}");
        }
        assert!(text.contains(".spitdag"), "{text}");
    }
    for args in [&["help", "dag"][..], &["dag", "--help"]] {
        let text = stdout(&spit(args));
        assert!(
            text.contains(
                "usage: spit dag <recipe.spitin> or <pipeline.spit> <inputs.spitout | ->"
            ),
            "{text}"
        );
        assert!(text.contains("-o <file>"), "{text}");
        assert!(text.contains("example:"), "{text}");
    }
    let version = spit(&["--version"]);
    assert_eq!(
        stdout(&version),
        format!("spit {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn check_compiles_the_pipeline_without_its_inputs() {
    let output = spit(&["check", "examples/types/typed.spit"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Pipeline valid.\n");
}

#[test]
fn dag_resolves_a_pipeline_over_a_spitout() {
    let output = spit(&[
        "dag",
        "examples/types/typed.spit",
        "examples/types/typed.spitout",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).starts_with("Job 1\n"));
}

#[test]
fn partial_dag_plans_the_complete_stores_and_records_the_rest() {
    let recipe = format!(
        "{}/tests/fixtures/weekly_stores/weekly.spitin",
        env!("CARGO_MANIFEST_DIR")
    );
    let failed = spit(&["dag", &recipe]);
    assert!(!failed.status.success());
    assert!(stderr(&failed).contains("9 more artifacts cannot be produced"));
    assert!(stderr(&failed).contains("spit dag --partial"));
    assert!(stderr(&failed)
        .contains("pricing[store=S07] exists; its `store` differs only in letter case"));
    assert!(stderr(&failed).contains("warning: source pricing[store=S07] is used by no job"));

    let partial = spit(&["dag", &recipe, "--partial", "--json"]);
    assert!(partial.status.success(), "{}", stderr(&partial));
    let json = stdout(&partial);
    assert_eq!(json.matches("\"operation\":").count(), 30);
    assert_eq!(json.matches("\"identity\":").count(), 9);
    assert!(json.contains("\"operation\":\"chain_summary\""));
    assert!(json.contains("\"identity\":\"report[store=s03]\""));
    assert!(json.contains("\"input `prices` needs") || json.contains("no `pricing` artifact"));
}

#[test]
fn partial_on_complete_inputs_has_the_same_jobs() {
    let files = ["examples/types/typed.spit", "examples/types/typed.spitout"];
    let full = spit(&["dag", files[0], files[1], "--json"]);
    let partial = spit(&["dag", files[0], files[1], "--partial", "--json"]);
    assert!(full.status.success() && partial.status.success());
    assert_eq!(stdout(&full), stdout(&partial));
}

#[test]
fn dag_with_paths_displays_resolved_paths_before_command_expansion() {
    let output = spit(&[
        "dag",
        "examples/commands/field_survey/field_survey.spit",
        "examples/commands/field_survey/field_survey.spitout",
        "--paths",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report = stdout(&output);
    assert_eq!(report.matches("Job ").count(), 93);
    assert!(report.contains("moving: ground_map[site=01,visit=01]"));
    assert!(report.contains("path: site-01/visit-01/map/site-01_visit-01_map.tif"));
}

#[test]
fn expanded_examples_resolve() {
    for (example, expected_jobs) in [
        ("examples/pipelines/branching", 21),
        ("examples/pipelines/complex", 25),
        ("examples/pipelines/rich_shapes", 17),
        ("examples/commands/field_survey/field_survey", 93),
        ("examples/analytics/analytics", 34),
    ] {
        let (pipeline, sources) = (format!("{example}.spit"), format!("{example}.spitout"));
        let output = spit(&["dag", &pipeline, &sources]);
        assert!(output.status.success(), "{pipeline}: {}", stderr(&output));
        assert!(
            stderr(&output).contains(&format!("note: {expected_jobs} jobs resolved.")),
            "{pipeline} resolved an unexpected number of jobs"
        );
    }
}

#[test]
fn path_rules_report_fallbacks_and_strict_paths_reject_them() {
    let rules = spit(&[
        "check",
        "examples/commands/field_survey/field_survey.spit",
        "--path-rules",
    ]);
    assert!(rules.status.success(), "{}", stderr(&rules));
    let report = stdout(&rules);
    assert!(report.contains("raw_photo (source): explicit"));
    assert!(report.contains("photo_response (output): default derivatives/{@product}/{@entities}.txt, `.txt` from operation `estimate_response`"));
    assert!(report.contains("vegetation (output): default"));

    for command in ["check", "dag"] {
        let mut args = vec![command, "examples/commands/field_survey/field_survey.spit"];
        if command == "dag" {
            args.push("examples/commands/field_survey/field_survey.spitout");
        }
        args.push("--strict-paths");
        let strict = spit(&args);
        assert!(!strict.status.success(), "{command}");
        assert!(
            stderr(&strict).contains("strict paths requires explicit rules"),
            "{command}"
        );
    }
}

#[test]
fn each_option_applies_to_its_commands() {
    let output = spit(&[
        "artifacts",
        "examples/commands/command_demo/command_demo.spit",
        "examples/commands/command_demo/command_demo.spitout",
        "--paths",
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).starts_with("error: --paths applies to dag\n"));
    let conflict = spit(&["dag", "a.spit", "b.spitout", "--json", "-o", "x"]);
    assert!(stderr(&conflict).starts_with("error: --json cannot be used with -o\n"));
    // The command lines are a text view; the .spitdag already holds them.
    let commands = spit(&["dag", "a.spit", "b.spitout", "--commands", "-o", "x"]);
    assert!(stderr(&commands).starts_with("error: --commands cannot be used with -o; run dag with --commands to inspect command lines, or with -o <file> to save a .spitdag\n"));
    let commands = spit(&["artifacts", "a.spit", "b.spitout", "--commands"]);
    assert!(stderr(&commands).starts_with("error: --commands applies to dag\n"));
    let extra = spit(&["check", "a.spit", "b.spitout"]);
    assert!(stderr(&extra).starts_with("error: unexpected file `b.spitout`\n"));
}

#[test]
fn check_json_reads_the_pipeline_file_and_dag_json_emits_the_spitdag() {
    let check = spit(&[
        "check",
        "examples/commands/command_demo/command_demo.spit",
        "--json",
    ]);
    assert!(check.status.success());
    // A clean pipeline also gives each output's path, which its default
    // rule writes nowhere in full, for an editor to show.
    assert_eq!(
        stdout(&check),
        "{\"diagnostics\":[],\"paths\":[\
         {\"product\":\"sorted\",\"line\":10,\"path\":\"sorted/{@entities}.txt\"},\
         {\"product\":\"merged\",\"line\":14,\"path\":\"merged/{@entities}.txt\"}]}\n"
    );
    let run = || {
        spit(&[
            "dag",
            "examples/commands/command_demo/command_demo.spit",
            "examples/commands/command_demo/command_demo.spitout",
            "--json",
        ])
    };
    let dag = run();
    assert!(dag.status.success(), "{}", stderr(&dag));
    let graph = stdout(&dag);
    assert!(graph.starts_with("{\"version\":4,\"generator\":{\"name\":\"spit\",\"version\":\""));
    // A `.spitout` alone says nothing of where its files are.
    assert!(
        graph.contains("\"root\":null,\"external_inputs\":["),
        "{graph}"
    );
    // What a full run leaves behind, and the one program it needs.
    assert!(
        graph.contains("\"targets\":[{\"product\":\"merged\""),
        "{graph}"
    );
    assert_eq!(graph.matches("{\"product\":\"merged\"").count(), 4);
    assert!(
        graph.contains("\"executables\":[\"sort\"],\"removed\":[],\"left_out\":[],\"jobs\":["),
        "{graph}"
    );
    assert!(
        graph.contains("\"depends_on\":[],\"dependents\":[4]"),
        "{graph}"
    );
    assert!(
        graph.contains("\"depends_on\":[1,2],\"dependents\":[]"),
        "{graph}"
    );
    assert_eq!(graph.matches("\"fingerprint\":\"").count(), 5);
    // A bound DAG: every artifact has its path and every job its command.
    assert!(graph.contains("\"path\":\"input/alpha/01.txt\""), "{graph}");
    assert!(graph.contains("\"command\":[[\"sort\"]"), "{graph}");
    assert!(graph.contains("\"product\":\"shard\",\"entities\":{\"group\":\"alpha\",\"part\":\"01\"},\"type\":{\"name\":\"Lines\",\"args\":[]}"));
    assert!(graph.contains("\"inputs\":{\"items\":["));
    assert!(graph.contains("\"depends_on\":[1,2]"));
    assert_eq!(graph.matches("\"operation\":").count(), 5);
    assert_eq!(graph, stdout(&run()));
}

#[test]
fn dag_json_names_every_output_port() {
    let output = spit(&[
        "dag",
        "examples/pipelines/selectors.spit",
        "examples/pipelines/selectors.spitout",
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let graph = stdout(&output);
    assert!(graph.contains("\"outputs\":{\"low\":{\"product\":\"low_band\""));
    assert!(graph.contains("\"high\":{\"product\":\"high_band\""));
}

#[test]
fn dag_json_stage_is_an_array_of_names() {
    let output = spit(&[
        "dag",
        "examples/stages/nested.spit",
        "examples/stages/nested.spitout",
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let graph = stdout(&output);
    assert!(graph.contains("\"stage\":[\"preprocess\",\"clean\"]"));
    assert!(!graph.contains("\"stage\":\"preprocess/clean\""));
}

#[test]
fn path_rules_show_a_missing_rule_and_strict_paths_accept_complete_rules() {
    let tree = Tree::new("cli-paths", &[]);
    let directory = tree.path();
    let file = directory.join("spit paths.spit");
    let pipeline =
        "source raw [id]\npath raw: input/{id}.txt\noperation copy(input)\nresult = copy(raw)\n";
    fs::write(&file, pipeline).unwrap();
    let missing = spit(&["check", file.to_str().unwrap(), "--path-rules"]);
    assert!(stdout(&missing).contains("result (output): MISSING"));

    fs::write(&file, format!("{pipeline}path result: output/{{id}}.txt\n")).unwrap();
    let complete = spit(&["check", file.to_str().unwrap(), "--strict-paths"]);
    assert!(complete.status.success(), "{}", stderr(&complete));
}

#[test]
fn recipe_path_rules_show_combined_coverage_and_origin() {
    let tree = Tree::new("cli-recipe-paths", &[]);
    let pipeline = tree.write(
        "analysis.spit",
        "source raw [id]\noperation copy(input)\nresult = copy(raw)\npath result: output/{id}.txt\n",
    );
    let recipe = tree.write(
        "data.spitin",
        "pipeline analysis.spit\npath raw: input/{id}.txt\n",
    );
    let output = spit(&[
        "check",
        recipe.to_str().unwrap(),
        "--path-rules",
        "--strict-paths",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report = stdout(&output);
    assert!(
        report.contains("raw (source): explicit input/{id}.txt (recipe)"),
        "{report}"
    );
    assert!(
        report.contains("result (output): explicit output/{id}.txt"),
        "{report}"
    );
    assert!(report.contains("Recipe valid."), "{report}");

    let pipeline_only = spit(&["check", pipeline.to_str().unwrap(), "--path-rules"]);
    assert!(pipeline_only.status.success(), "{}", stderr(&pipeline_only));
    assert!(stdout(&pipeline_only).contains("raw (source): no rule (a recipe may supply one)"));

    tree.write(
        "analysis.spit",
        "source raw [id]\noperation copy(input)\nresult = copy(raw)\n",
    );
    let missing = spit(&[
        "check",
        recipe.to_str().unwrap(),
        "--path-rules",
        "--strict-paths",
    ]);
    assert!(!missing.status.success());
    assert!(stdout(&missing).contains("result (output): MISSING"));
    assert!(stderr(&missing).contains("no path rule for products: result"));
}

#[test]
fn a_message_names_the_file_it_is_about_when_that_file_was_not_given() {
    let tree = Tree::new("cli-file-names", &["in/1.txt", "in/2.txt", "ref/2.txt"]);
    let directory = tree.path();
    let pipeline = directory.join("pipeline.spit");
    // Line 6 joins `raw` to `ref`, which has no artifact at [id=1].
    fs::write(
        &pipeline,
        "source raw [id]\npath raw: in/{id}.txt\nsource ref [id]\npath ref: ref/{id}.txt\n\
         operation link(raw, ref)\nlinked = link(raw, ref)\n",
    )
    .unwrap();
    let recipe = directory.join("data.spitin");
    fs::write(&recipe, "pipeline pipeline.spit\n").unwrap();
    let shown = pipeline.display().to_string();

    // Given the recipe, a failed match in the pipeline names the pipeline.
    let dag = spit(&["dag", recipe.to_str().unwrap()]);
    assert!(!dag.status.success());
    assert!(
        stderr(&dag).contains(&format!(
            "error: {shown}: line 6, column 20: no `ref` artifact"
        )),
        "{}",
        stderr(&dag)
    );

    // Checking the recipe names the pipeline too, and keeps the column.
    fs::write(
        &pipeline,
        "source raw [id]\noperation link(raw)\nlinked = link(rwa)\n",
    )
    .unwrap();
    let check = spit(&["check", recipe.to_str().unwrap()]);
    assert_eq!(
        stderr(&check),
        format!("error: {shown}: line 3, column 15: unknown product `rwa`\n")
    );

    // Given the pipeline and records, the pipeline is the file given first;
    // the records are named, as their lines are in another file.
    fs::write(
        &pipeline,
        "source raw [id]\noperation link(raw)\nlinked = link(raw)\n",
    )
    .unwrap();
    let records = directory.join("inputs.spitout");
    fs::write(&records, "sources:\n    rwa[id=1]\n").unwrap();
    let dag = spit(&["dag", pipeline.to_str().unwrap(), records.to_str().unwrap()]);
    assert!(
        stderr(&dag).starts_with(&format!(
            "error: {}: line 2, column 5: unknown product `rwa`",
            records.display()
        )),
        "{}",
        stderr(&dag)
    );
    assert!(!stderr(&dag).contains(&shown), "{}", stderr(&dag));
}

#[test]
fn check_prints_every_diagnostic_and_fails_only_on_errors() {
    let tree = Tree::new("cli-diagnostics", &[]);
    let directory = tree.path();
    let broken = directory.join("broken.spit");
    fs::write(
        &broken,
        "source raw [id, batch]\npath: {@product}/{@entities}.txt\npath raw: in/{id}.txt\noperation clean(input)\ncleaned = clean(rwa)\n",
    )
    .unwrap();
    let warned = directory.join("warned.spit");
    fs::write(
        &warned,
        "source raw [id]\nsource spare [id]\noperation clean(input)\ncleaned = clean(raw)\n",
    )
    .unwrap();
    let broken_check = spit(&["check", broken.to_str().unwrap()]);
    let warned_check = spit(&["check", warned.to_str().unwrap()]);
    let warned_dag = spit(&["dag", warned.to_str().unwrap()]);

    assert!(!broken_check.status.success());
    assert_eq!(
        stderr(&broken_check),
        "warning: line 1, column 8: source product `raw` is never used as an input\nerror: line 3, column 11: path template for `raw` omits dimension `batch`; artifacts differing only in `batch` would share a path\nerror: line 5, column 17: unknown product `rwa`\n"
    );

    // Check compiles the pipeline alone; dag needs the dataset's inputs.
    assert!(warned_check.status.success());
    assert_eq!(
        stderr(&warned_check),
        "warning: line 2, column 8: source product `spare` is never used as an input\n"
    );
    assert_eq!(stdout(&warned_check), "Pipeline valid.\n");
    assert!(!warned_dag.status.success());
    assert!(
        stderr(&warned_dag).starts_with("error: dag needs a pipeline before"),
        "{}",
        stderr(&warned_dag)
    );
}

#[test]
fn a_recipe_names_its_own_pipeline_for_dag_and_artifacts() {
    let recipe = "examples/commands/command_demo/command_demo.spitin";
    let pipeline = "examples/commands/command_demo/command_demo.spit";
    let inventory = "examples/commands/command_demo/command_demo.spitout";
    // The example keeps no data beside its recipe; scan a folder that has
    // the shards its inventory lists.
    let data = Tree::new(
        "recipe-alone",
        &[
            "input/alpha/01.txt",
            "input/alpha/02.txt",
            "input/beta/01.txt",
        ],
    );
    let root = data.path().to_str().unwrap();
    for command in ["dag", "artifacts"] {
        let alone = spit(&[command, recipe, "--root", root]);
        assert!(alone.status.success(), "{}", stderr(&alone));
        // The pipeline is the recipe's to name, not the command line's, even
        // when it names the same one.
        let named = spit(&[command, pipeline, recipe]);
        assert!(!named.status.success(), "{command}");
        assert_eq!(
            stderr(&named),
            format!(
                "error: `{recipe}` names its own pipeline; run `spit {command} {recipe}` without `{pipeline}`\n"
            )
        );
        // A .spitout names none, so it takes the pipeline.
        let settled = spit(&[command, pipeline, inventory]);
        assert!(settled.status.success(), "{}", stderr(&settled));
    }
    let spitout = spit(&["dag", inventory]);
    assert!(!spitout.status.success());
    assert!(
        stderr(&spitout).starts_with(
            "error: dag needs a pipeline before `examples/commands/command_demo/command_demo.spitout`"
        ),
        "{}",
        stderr(&spitout)
    );
}

#[test]
fn a_recipe_run_in_memory_prints_each_pipeline_warning_once() {
    let tree = Tree::new("warn-once", &["in/a.txt"]);
    tree.write(
        "analysis.spit",
        "source raw : Raw [id]\nsource spare : Raw [id]\npath raw: in/{id}.txt\n\
         path spare: sp/{id}.txt\npath: out/{@product}/{id}.txt\n\
         operation clean(raw: Raw) -> Clean\ncommand clean: tool {raw} {output}\ncleaned = clean(raw)\n",
    );
    let recipe = tree.write("data.spitin", "pipeline analysis.spit\n");
    for command in ["inputs", "dag", "artifacts"] {
        let output = spit(&[command, recipe.to_str().unwrap()]);
        let errors = stderr(&output);
        assert!(output.status.success(), "{command}: {errors}");
        assert_eq!(
            errors.matches("`spare` is never used").count(),
            1,
            "{command}: {errors}"
        );
    }
}

#[test]
fn a_recipe_checks_a_pipeline_saved_with_a_byte_order_mark() {
    let tree = Tree::new("bom-recipe", &[]);
    tree.write(
        "analysis.spit",
        "\u{feff}source raw [id]\npath raw: in/{id}.txt\n",
    );
    let recipe = tree.write("data.spitin", "\u{feff}pipeline analysis.spit\n");
    let output = spit(&["check", recipe.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Recipe valid.\n");
}

#[test]
fn drop_rules_that_remove_every_group_stop_each_command() {
    let tree = Tree::new(
        "drop-all",
        &["data/sub-1/ses-1/image.nii", "data/sub-2/ses-1/image.nii"],
    );
    tree.write(
        "analysis.spit",
        "source image : Img [sub, ses]\npath image: data/sub-{sub}/ses-{ses}/image.nii\n\
         operation clean(img: Img) -> Clean\ncommand clean: tool {img} {output}\n\
         path: out/{@product}/{sub}_{ses}.txt\ncleaned = clean(image)\n",
    );
    // Every subject has one session, so the drop removes every subject;
    // before, a `require` after it checked nothing and passed.
    let recipe = tree.write(
        "data.spitin",
        "pipeline analysis.spit\ndiscover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         drop [sub] where sessions count<2\nrequire sessions count>=1 per [sub]\n",
    );
    for command in ["inputs", "dag", "artifacts"] {
        let output = spit(&[command, recipe.to_str().unwrap()]);
        assert!(!output.status.success(), "{command}");
        assert!(
            stderr(&output).contains(
                "error: drop rules removed all 2 [sub] groups, leaving nothing to plan: \
                 `drop [sub] where sessions count<2` (line 3)"
            ),
            "{command}: {}",
            stderr(&output)
        );
    }
}
