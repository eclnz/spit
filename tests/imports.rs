mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use spit::{diagnose_at, parse_pipeline_at, parse_source_inventory, resolve};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "spit-imports-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unqualified_and_nested_imports_work_in_sectioned_files() {
    let dir = TestDir::new();
    dir.write(
        "base.spit",
        "operation clean(one)\ncommand clean: cp {input} {output}\n",
    );
    dir.write("middle.spit", "use clean from base.spit as prep\n");
    let main = dir.write(
        "main.spit",
        "use prep::clean from middle.spit as stage\n\
         use clean from base.spit\n\
         products:\n  raw [id]\n  middle [id]\n  final [id]\n\
         pipeline:\n  middle = clean(raw)\n  final = stage::prep::clean(middle)\n\
         sources:\n  raw[id=x]\n",
    );
    let text = fs::read_to_string(&main).unwrap();
    let (pipeline, inventory) = support::parse_fixture_at(&text, &main).unwrap();
    assert_eq!(pipeline.operations.len(), 2);
    assert_eq!(pipeline.commands.len(), 2);
    assert_eq!(
        resolve(&pipeline, &inventory.unwrap()).unwrap().jobs.len(),
        2
    );
}

#[test]
fn import_errors_point_to_the_use_line() {
    let dir = TestDir::new();
    dir.write("base.spit", "operation clean(one)\n");
    let main = dir.write("main.spit", "use absent from base.spit\n");
    let error = support::parse_fixture_at(&fs::read_to_string(&main).unwrap(), &main).unwrap_err();
    assert_eq!(error.line(), 1);
    assert!(error.message.contains("not a source or operation"));

    let main = dir.write(
        "main.spit",
        "use clean from base.spit\nuse clean from base.spit\n",
    );
    let error = support::parse_fixture_at(&fs::read_to_string(&main).unwrap(), &main).unwrap_err();
    assert_eq!(error.line(), 2);
    assert!(error.message.contains("conflicts with operation"));

    dir.write(
        "base.spit",
        "use clean from main.spit\noperation clean(one)\n",
    );
    let error = support::parse_fixture_at(&fs::read_to_string(&main).unwrap(), &main).unwrap_err();
    assert_eq!(error.line(), 1);
    assert!(error.message.contains("import cycle"));
}

#[test]
fn diagnostics_resolve_imports_using_pipeline_location() {
    let dir = TestDir::new();
    dir.write("base.spit", "operation clean(one)\n");
    let main = dir.write(
        "main.spit",
        "source raw [id]\nuse clean from base.spit as prep\nresult = prep::clean(raw)\n",
    );
    let text = fs::read_to_string(&main).unwrap();
    assert!(diagnose_at(&text, None, &main).is_empty());
    let broken = text.replace("base.spit", "missing.spit");
    let errors = diagnose_at(&broken, None, &main);
    assert_eq!(errors[0].line, Some(2));
    assert!(errors[0].message.contains("cannot load import"));
}

#[test]
fn an_imported_source_keeps_its_path_rule_under_its_alias() {
    let dir = TestDir::new();
    dir.write(
        "base.spit",
        "path: input/{product}/{id}.txt\nsource raw [id]\noperation clean(one)\n",
    );
    let main = dir.write(
        "main.spit",
        "use raw, clean from base.spit as lib\nresult = lib::clean(lib::raw)\n",
    );
    let pipeline = parse_pipeline_at(&fs::read_to_string(&main).unwrap(), &main).unwrap();
    assert_eq!(pipeline.product_paths["lib::raw"], "input/raw/{id}.txt");
    let inventory = parse_source_inventory("sources:\n  lib::raw[id=x]\n").unwrap();
    assert_eq!(resolve(&pipeline, &inventory).unwrap().jobs.len(), 1);
}

#[test]
fn quoted_import_path_can_contain_as() {
    let dir = TestDir::new();
    dir.write("base as draft.spit", "operation clean(one)\n");
    let main = dir.write(
        "main.spit",
        "use clean from \"base as draft.spit\" as prep\nsource raw [id]\nresult = prep::clean(raw)\n",
    );
    let (pipeline, _) =
        support::parse_fixture_at(&fs::read_to_string(&main).unwrap(), &main).unwrap();
    assert_eq!(pipeline.operations[0].name, "prep::clean");
}

#[test]
fn import_all_brings_definitions_but_not_steps() {
    let dir = TestDir::new();
    dir.write(
        "base.spit",
        "source raw [id]\noperation clean(one)\ncommand clean: cp {input} {output}\ncleaned = clean(raw)\n",
    );
    let main = dir.write(
        "main.spit",
        "use base.spit as lib\nresult = lib::clean(lib::raw)\n",
    );
    let pipeline = parse_pipeline_at(&fs::read_to_string(&main).unwrap(), &main).unwrap();
    let inventory = parse_source_inventory("sources:\n  lib::raw[id=new]\n").unwrap();
    assert_eq!(pipeline.products.len(), 2);
    assert_eq!(pipeline.products[0].name, "lib::raw");
    assert_eq!(pipeline.operations[0].name, "lib::clean");
    assert_eq!(pipeline.commands[0].operation, "lib::clean");
    assert_eq!(pipeline.invocations.len(), 1);
    assert_eq!(inventory.artifacts.len(), 1);
    assert_eq!(resolve(&pipeline, &inventory).unwrap().jobs.len(), 1);

    let unqualified = dir.write("unqualified.spit", "use base.spit\nresult = clean(raw)\n");
    let pipeline =
        parse_pipeline_at(&fs::read_to_string(&unqualified).unwrap(), &unqualified).unwrap();
    let inventory = parse_source_inventory("sources:\n  raw[id=new]\n").unwrap();
    assert_eq!(pipeline.operations[0].name, "clean");
    assert_eq!(resolve(&pipeline, &inventory).unwrap().jobs.len(), 1);
}

#[test]
fn imported_definitions_are_not_reported_as_unused() {
    let path = Path::new("examples/imports/imported.spit");
    let text = fs::read_to_string(path).unwrap();
    assert!(diagnose_at(&text, None, path).is_empty());
}

#[test]
fn an_imported_path_keeps_escaped_braces() {
    let dir = TestDir::new();
    dir.write(
        "base.spit",
        "path: input/{{product}}/{product}/{id}.txt\nsource raw [id]\n",
    );
    let main = dir.write("main.spit", "use raw from base.spit as lib\n");
    let (pipeline, _) =
        support::parse_fixture_at(&fs::read_to_string(&main).unwrap(), &main).unwrap();
    // Only the placeholder takes the product's name; the escaped braces stay literal.
    assert_eq!(
        pipeline.product_paths["lib::raw"],
        "input/{{product}}/raw/{id}.txt"
    );
}
