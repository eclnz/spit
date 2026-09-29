use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use spit::{
    parse_document, parse_pipeline, parse_source_inventory, render_bash, resolve, CommandTemplate,
    PathTemplate,
};

fn demo_script() -> String {
    let (pipeline, embedded) =
        parse_document(include_str!("../examples/commands/bash_demo.spit")).unwrap();
    assert!(embedded.is_none());
    let inventory =
        parse_source_inventory(include_str!("../examples/commands/bash_demo.spitout")).unwrap();
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
    let (mut pipeline, _) =
        parse_document(include_str!("../examples/commands/bash_demo.spit")).unwrap();
    let inventory =
        parse_source_inventory(include_str!("../examples/commands/bash_demo.spitout")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();

    pipeline.commands[0].template = CommandTemplate::parse("sort -o {output} {missing}").unwrap();
    assert!(render_bash(&pipeline, &dag)
        .unwrap_err()
        .to_string()
        .contains("unknown placeholder"));

    pipeline.commands[0].template = CommandTemplate::parse("sort -o {output} {input}").unwrap();
    pipeline.path_template = Some(PathTemplate::parse("same.txt").unwrap());
    pipeline.product_paths.clear();
    assert!(render_bash(&pipeline, &dag)
        .unwrap_err()
        .to_string()
        .contains("omits dimension"));
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
    let (mut pipeline, _) =
        parse_document(include_str!("../examples/commands/bash_demo.spit")).unwrap();
    let inventory =
        parse_source_inventory(include_str!("../examples/commands/bash_demo.spitout")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    pipeline.commands[1].template =
        CommandTemplate::parse("sort -o {output} --files={inputs}").unwrap();
    assert!(render_bash(&pipeline, &dag)
        .unwrap_err()
        .to_string()
        .contains("must be a complete command argument"));
}

#[test]
fn adding_a_group_to_inventory_expands_the_script() {
    let (pipeline, _) =
        parse_document(include_str!("../examples/commands/bash_demo.spit")).unwrap();
    let inventory = parse_source_inventory(&format!(
        "{}    shard[group=gamma,part=01]\n",
        include_str!("../examples/commands/bash_demo.spitout")
    ))
    .unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 7);
    let script = render_bash(&pipeline, &dag).unwrap();
    assert!(script.contains("input/gamma/01.txt"));
    assert!(script.contains("merged/group=gamma.txt"));
}

#[test]
fn field_survey_generates_valid_bash_for_new_visits() {
    let (pipeline, embedded) =
        parse_document(include_str!("../examples/commands/field_survey.spit")).unwrap();
    assert!(embedded.is_none());
    let inventory =
        parse_source_inventory(include_str!("../examples/commands/field_survey.spitout")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 93);
    let script = render_bash(&pipeline, &dag).unwrap();
    assert!(script.contains("'--pose'"));
    assert!(script.contains("'--meta'"));
    let import = script
        .lines()
        .find(|line| {
            line.starts_with("'imgconvert'")
                && line.contains("photo_img/site=01__visit=01__shot=01")
        })
        .unwrap();
    let positions = [
        "site-01_visit-01_shot-01_photo.raw",
        "'--pose'",
        "site-01_visit-01_shot-01_photo.gpx",
        "site-01_visit-01_shot-01_photo.imu",
        "'--meta'",
        "site-01_visit-01_shot-01_photo.json",
    ]
    .map(|part| import.find(part).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(script.contains("'imgstack'"));
    assert!(script.contains("'imgalign' '-in'"));
    assert!(script.contains("'xfmconvert'"));
    assert!(script.contains("'imgresample'"));
    assert!(script.contains("'--interp' 'nearest'"));
    assert!(script.contains("'yieldtable'"));
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
        "{}    raw_photo[site=03,visit=01,shot=01]\n    raw_photo[site=03,visit=01,shot=02]\n    photo_gps[site=03,visit=01,shot=01]\n    photo_gps[site=03,visit=01,shot=02]\n    photo_imu[site=03,visit=01,shot=01]\n    photo_imu[site=03,visit=01,shot=02]\n    photo_json[site=03,visit=01,shot=01]\n    photo_json[site=03,visit=01,shot=02]\n    flat_field[site=03,visit=01]\n    flat_field_json[site=03,visit=01]\n    ground_map[site=03,visit=01]\n",
        include_str!("../examples/commands/field_survey.spitout")
    ))
    .unwrap();
    let expanded = resolve(&pipeline, &inventory).unwrap();
    assert!(expanded.jobs.len() > dag.jobs.len());
    let script = render_bash(&pipeline, &expanded).unwrap();
    assert!(script.contains("site-03/visit-01/photos/site-03_visit-01_shot-01_photo.raw"));
    assert!(script.contains("derivatives/yield_table/site=03__visit=01.csv"));
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
fn command_uses_executable_on_path() {
    let text = "source raw [id]\npath raw: input/{id}.txt\npath result: output/{id}.txt\noperation copy(data: one)\ncommand copy: copy_data {data} {output}\nresult = copy(raw)\nsources:\n  raw[id=x]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    let script = render_bash(&pipeline, &dag).unwrap();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("spit path command {} {suffix}", std::process::id()));
    fs::create_dir_all(root.join("input")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("input/x.txt"), "hello\n").unwrap();
    let executable = root.join("bin/copy_data");
    fs::write(&executable, "#!/bin/sh\ncp \"$1\" \"$2\"\n").unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    let path = format!(
        "{}:{}",
        root.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("SPIT_ROOT", &root)
        .env("PATH", path)
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

#[test]
fn backslashes_follow_bash_quoting_rules() {
    let (mut pipeline, _) =
        parse_document(include_str!("../examples/commands/bash_demo.spit")).unwrap();
    let inventory =
        parse_source_inventory(include_str!("../examples/commands/bash_demo.spitout")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    pipeline.commands[0].template =
        CommandTemplate::parse(r#"tool "a\b" "q\"x" "s\\t" c\d 'e\f' {input} {output}"#).unwrap();
    let script = render_bash(&pipeline, &dag).unwrap();
    assert!(
        script.contains(r#"'tool' 'a\b' 'q"x' 's\t' 'cd' 'e\f' "#),
        "{script}"
    );
}

#[test]
fn a_collection_expands_in_natural_order() {
    let text = "\
source frame [subject, run]
path: {product}/{subject}/{run}.txt
path stacked: {product}/{subject}.txt
operation stack(frames: many Frame) -> Stack @ drop(run)
command stack: stack {frames} {output}
stacked = stack(frame @ vary(run))
";
    let pipeline = parse_pipeline(text).unwrap();
    let inventory = parse_source_inventory(
        "sources:\n  frame[subject=s10,run=10]\n  frame[subject=s10,run=2]\n  frame[subject=s10,run=1]\n",
    )
    .unwrap();
    let script = render_bash(&pipeline, &resolve(&pipeline, &inventory).unwrap()).unwrap();
    assert!(script.contains(
        "'stack' \"$SPIT_ROOT\"/'frame/s10/1.txt' \"$SPIT_ROOT\"/'frame/s10/2.txt' \"$SPIT_ROOT\"/'frame/s10/10.txt'"
    ));
}
