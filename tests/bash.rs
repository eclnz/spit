use std::fs;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use spit::{
    inspect_paths, parse_document, parse_pipeline, parse_source_inventory, render_bash,
    render_bound_dag, resolve, PathRule,
};

fn demo_script() -> String {
    let (pipeline, embedded) = parse_document(include_str!("../examples/commands/bash_demo.spit")).unwrap();
    assert!(embedded.is_none());
    let inventory = parse_source_inventory(include_str!("../examples/commands/bash_demo.sources")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 5);
    render_bash(&pipeline, &dag).unwrap()
}

#[test]
fn generated_script_uses_inventory_groups_and_declared_arguments() {
    let script = demo_script();
    assert!(script.contains("'sort' '-m' '-u' '-o'"));
    assert!(script.contains("input/alpha/01.txt"));
    assert!(script.contains("input/beta/01.txt"));

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("spit bash {} {suffix}", std::process::id()));
    fs::create_dir_all(root.join("input/alpha")).unwrap();
    fs::create_dir_all(root.join("input/beta")).unwrap();
    fs::write(root.join("input/alpha/01.txt"), "pear\napple\napple\n").unwrap();
    fs::write(root.join("input/alpha/02.txt"), "banana\npear\n").unwrap();

    let missing = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("SPIT_ROOT", &root)
        .stdout(Stdio::null())
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("input/beta/01.txt"));

    fs::write(root.join("input/beta/01.txt"), "zeta\neta\n").unwrap();
    let run = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("SPIT_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join("merged/group=alpha.txt")).unwrap(),
        "apple\nbanana\npear\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("merged/group=beta.txt")).unwrap(),
        "eta\nzeta\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn backend_rejects_undeclared_placeholders_and_path_collisions() {
    let (mut pipeline, _) = parse_document(include_str!("../examples/commands/bash_demo.spit")).unwrap();
    let inventory = parse_source_inventory(include_str!("../examples/commands/bash_demo.sources")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();

    pipeline.commands[0].template = "sort -o {output} {missing}".to_owned();
    assert!(render_bash(&pipeline, &dag)
        .unwrap_err()
        .to_string()
        .contains("unknown placeholder"));

    pipeline.commands[0].template = "sort -o {output} {input}".to_owned();
    pipeline.path_template = Some("same.txt".to_owned());
    pipeline.product_paths.clear();
    assert!(render_bash(&pipeline, &dag)
        .unwrap_err()
        .to_string()
        .contains("same path"));
}

#[test]
fn sectioned_commands_bind_positional_inputs() {
    let text = "products:\n  left : Data [id]\n  right : Data [id]\n  result : Data [id]\n\
operations:\n  join(Data, Data) -> Data\n\
pipeline:\n  result = join(left, right)\n\
commands:\n  join: tool --left={input1} --right {input2} --out {output}\n\
path: {product}/{entities}.txt\n";
    let pipeline = parse_pipeline(text).unwrap();
    let inventory = parse_source_inventory("sources:\n  left[id=x]\n  right[id=x]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    let script = render_bash(&pipeline, &dag).unwrap();
    assert!(script.contains("'--left='\"$SPIT_ROOT\"/'left/id=x.txt'"));
    assert!(script.contains("'--right' \"$SPIT_ROOT\"/'right/id=x.txt'"));
    assert!(script.contains("'--out' \"$SPIT_ROOT\"/'result/id=x.txt'"));
}

#[test]
fn many_input_must_occupy_its_own_argument() {
    let (mut pipeline, _) = parse_document(include_str!("../examples/commands/bash_demo.spit")).unwrap();
    let inventory = parse_source_inventory(include_str!("../examples/commands/bash_demo.sources")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    pipeline.commands[1].template = "sort -o {output} --files={inputs}".to_owned();
    assert!(render_bash(&pipeline, &dag)
        .unwrap_err()
        .to_string()
        .contains("must be a complete command argument"));
}

#[test]
fn adding_a_group_to_inventory_expands_the_script() {
    let (pipeline, _) = parse_document(include_str!("../examples/commands/bash_demo.spit")).unwrap();
    let inventory = parse_source_inventory(&format!(
        "{}    shard[group=gamma,part=01]\n",
        include_str!("../examples/commands/bash_demo.sources")
    ))
    .unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 7);
    let script = render_bash(&pipeline, &dag).unwrap();
    assert!(script.contains("input/gamma/01.txt"));
    assert!(script.contains("merged/group=gamma.txt"));
}

#[test]
fn act_example_generates_valid_bash_for_new_sessions() {
    let (pipeline, embedded) =
        parse_document(include_str!("../examples/commands/mrtrix3_act.spit")).unwrap();
    assert!(embedded.is_none());
    let inventory =
        parse_source_inventory(include_str!("../examples/commands/mrtrix3_act.sources")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 93);
    let script = render_bash(&pipeline, &dag).unwrap();
    assert!(script.contains("'-fslgrad'"));
    assert!(script.contains("'-json_import'"));
    let import = script
        .lines()
        .find(|line| {
            line.starts_with("'mrconvert'") && line.contains("dwi_mif/sub=01__ses=01__run=01")
        })
        .unwrap();
    let positions = [
        "sub-01_ses-01_run-01_dwi.nii.gz",
        "'-fslgrad'",
        "sub-01_ses-01_run-01_dwi.bvec",
        "sub-01_ses-01_run-01_dwi.bval",
        "'-json_import'",
        "sub-01_ses-01_run-01_dwi.json",
    ]
    .map(|part| import.find(part).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(script.contains("'dwicat'"));
    assert!(script.contains("'flirt' '-in'"));
    assert!(script.contains("'transformconvert'"));
    assert!(script.contains("'mrtransform'"));
    assert!(script.contains("'-interp' 'nearest'"));
    assert!(script.contains("'tck2connectome'"));
    let syntax = Command::new("bash")
        .arg("-n")
        .arg("-c")
        .arg(&script)
        .output()
        .unwrap();
    assert!(
        syntax.status.success(),
        "{}",
        String::from_utf8_lossy(&syntax.stderr)
    );

    let inventory = parse_source_inventory(&format!(
        "{}    raw_dwi[sub=03,ses=01,run=01]\n    raw_dwi[sub=03,ses=01,run=02]\n    dwi_bvec[sub=03,ses=01,run=01]\n    dwi_bvec[sub=03,ses=01,run=02]\n    dwi_bval[sub=03,ses=01,run=01]\n    dwi_bval[sub=03,ses=01,run=02]\n    dwi_json[sub=03,ses=01,run=01]\n    dwi_json[sub=03,ses=01,run=02]\n    reverse_b0[sub=03,ses=01]\n    reverse_b0_json[sub=03,ses=01]\n    t1w[sub=03,ses=01]\n",
        include_str!("../examples/commands/mrtrix3_act.sources")
    ))
    .unwrap();
    let expanded = resolve(&pipeline, &inventory).unwrap();
    assert!(expanded.jobs.len() > dag.jobs.len());
    let script = render_bash(&pipeline, &expanded).unwrap();
    assert!(script.contains("sub-03/ses-01/dwi/sub-03_ses-01_run-01_dwi.nii.gz"));
    assert!(script.contains("derivatives/weighted_connectome/sub=03__ses=01.csv"));
}

#[test]
fn path_coverage_exposes_default_fallbacks_and_strict_rejects_them() {
    let (pipeline, _) = parse_document(include_str!("../examples/commands/mrtrix3_act.spit")).unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    assert!(coverage
        .entries
        .iter()
        .any(|entry| { entry.product == "wm_fod" && matches!(entry.rule, PathRule::Default(_)) }));
    assert!(coverage.entries.iter().any(|entry| {
        entry.product == "wm_response" && matches!(entry.rule, PathRule::Explicit(_))
    }));
    coverage.validate(false).unwrap();
    assert!(coverage
        .validate(true)
        .unwrap_err()
        .to_string()
        .contains("wm_fod"));
}

#[test]
fn path_coverage_catches_missing_and_invalid_rules_without_jobs() {
    let mut pipeline = parse_pipeline("source unused [id]\n").unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    assert_eq!(coverage.entries[0].rule, PathRule::Missing);
    assert!(coverage.validate(false).is_err());

    pipeline
        .product_paths
        .insert("unused".to_owned(), "input/{missing}.txt".to_owned());
    assert!(inspect_paths(&pipeline)
        .unwrap_err()
        .to_string()
        .contains("absent dimension"));

    pipeline
        .product_paths
        .insert("unused".to_owned(), "input/{id}.txt".to_owned());
    inspect_paths(&pipeline).unwrap().validate(true).unwrap();
}

#[test]
fn bound_dag_shows_port_names_and_paths_without_commands() {
    let (mut pipeline, _) = parse_document(include_str!("../examples/commands/mrtrix3_act.spit")).unwrap();
    let inventory =
        parse_source_inventory(include_str!("../examples/commands/mrtrix3_act.sources")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    pipeline.commands.clear();
    let report = render_bound_dag(&pipeline, &dag).unwrap();
    assert_eq!(report.matches("Job ").count(), 93);
    assert!(report.contains("moving: t1w[sub=01,ses=01]"));
    assert!(report.contains("reference: session_b0_nifti[sub=01,ses=01]"));
    assert!(report.contains("path: derivatives/weighted_connectome/sub=01__ses=01.csv"));

    pipeline.product_paths.remove("weighted_connectome");
    pipeline.path_template = None;
    assert!(render_bound_dag(&pipeline, &dag)
        .unwrap_err()
        .to_string()
        .contains("no path rule"));
}

#[test]
fn named_many_port_expands_in_entity_order_as_separate_arguments() {
    let text = "source raw [group, part]\npath: {product}/{entities}.txt\noperation gather(items: many) @ drop(part)\ncommand gather: collect {items} {output}\nresult = gather(raw @ vary(part))\nsources:\n  raw[group=a,part=2]\n  raw[group=a,part=1]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    let script = render_bash(&pipeline, &dag).unwrap();
    let command = script
        .lines()
        .find(|line| line.starts_with("'collect'"))
        .unwrap();
    assert!(command.contains("'raw/group=a__part=1.txt' \"$SPIT_ROOT\"/'raw/group=a__part=2.txt'"));
    assert!(command.ends_with("\"$SPIT_ROOT\"/'result/group=a.txt'"));
}

#[test]
fn declared_shell_source_provides_a_callable_function() {
    let text = "source raw [id]\npath raw: input/{id}.txt\npath result: output/{id}.txt\nshell-source: scripts/functions.sh\noperation copy(data: one)\ncommand copy: copy_data {data} {output}\nresult = copy(raw)\nsources:\n  raw[id=x]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    let script = render_bash(&pipeline, &dag).unwrap();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "spit sourced function {} {suffix}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("input")).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("input/x.txt"), "hello\n").unwrap();
    fs::write(
        root.join("scripts/functions.sh"),
        "copy_data() { cp -- \"$1\" \"$2\"; }\n",
    )
    .unwrap();
    let run = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("SPIT_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join("output/x.txt")).unwrap(),
        "hello\n"
    );
    fs::remove_dir_all(root).unwrap();
}
