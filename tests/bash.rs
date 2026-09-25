use std::fs;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use spit::{parse_document, parse_pipeline, parse_source_inventory, render_bash, resolve};

fn demo_script() -> String {
    let (pipeline, embedded) = parse_document(include_str!("../examples/bash_demo.spit")).unwrap();
    assert!(embedded.is_none());
    let inventory = parse_source_inventory(include_str!("../examples/bash_demo.sources")).unwrap();
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
    let (mut pipeline, _) = parse_document(include_str!("../examples/bash_demo.spit")).unwrap();
    let inventory = parse_source_inventory(include_str!("../examples/bash_demo.sources")).unwrap();
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
    let (mut pipeline, _) = parse_document(include_str!("../examples/bash_demo.spit")).unwrap();
    let inventory = parse_source_inventory(include_str!("../examples/bash_demo.sources")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    pipeline.commands[1].template = "sort -o {output} --files={inputs}".to_owned();
    assert!(render_bash(&pipeline, &dag)
        .unwrap_err()
        .to_string()
        .contains("must be a complete command argument"));
}

#[test]
fn adding_a_group_to_inventory_expands_the_script() {
    let (pipeline, _) = parse_document(include_str!("../examples/bash_demo.spit")).unwrap();
    let inventory = parse_source_inventory(&format!(
        "{}    shard[group=gamma,part=01]\n",
        include_str!("../examples/bash_demo.sources")
    ))
    .unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 7);
    let script = render_bash(&pipeline, &dag).unwrap();
    assert!(script.contains("input/gamma/01.txt"));
    assert!(script.contains("merged/group=gamma.txt"));
}
