mod support;

use support::Tree;

use std::fs;
use std::path::Path;

use spit::{diagnose_in, parse_pipeline_at, parse_source_inventory, resolve, Context};

#[test]
fn unqualified_and_nested_imports_work_together() {
    let dir = Tree::new("imports", &[]);
    dir.write(
        "base.spit",
        "operation clean(input)\ncommand clean: cp {input} {@output}\n",
    );
    dir.write("middle.spit", "use clean from base.spit as prep\n");
    let main = dir.write(
        "main.spit",
        "use prep::clean from middle.spit as stage\n\
         use clean from base.spit\n\
         source raw [id]\n\
         middle = clean(raw)\n\
         final = stage::prep::clean(middle)\n\
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
    let dir = Tree::new("imports", &[]);
    dir.write("base.spit", "operation clean(input)\n");
    let main = dir.write("main.spit", "use absent from base.spit\n");
    let error = support::parse_fixture_at(&fs::read_to_string(&main).unwrap(), &main).unwrap_err();
    assert_eq!(error.line(), 1);
    assert!(error
        .message()
        .contains("not a source, operation, sidecars group or check"));

    let main = dir.write(
        "main.spit",
        "use clean from base.spit\nuse clean from base.spit\n",
    );
    let error = support::parse_fixture_at(&fs::read_to_string(&main).unwrap(), &main).unwrap_err();
    assert_eq!(error.line(), 2);
    assert!(error.message().contains("conflicts with operation"));

    dir.write(
        "base.spit",
        "use clean from main.spit\noperation clean(input)\n",
    );
    let error = support::parse_fixture_at(&fs::read_to_string(&main).unwrap(), &main).unwrap_err();
    assert_eq!(error.line(), 1);
    assert!(error.message().contains("import cycle"));
}

#[test]
fn diagnostics_resolve_imports_using_pipeline_location() {
    let dir = Tree::new("imports", &[]);
    dir.write("base.spit", "operation clean(input)\n");
    let main = dir.write(
        "main.spit",
        "source raw [id]\nuse clean from base.spit as prep\nresult = prep::clean(raw)\n",
    );
    let text = fs::read_to_string(&main).unwrap();
    assert!(diagnose_in(&text, None, Context::at(&main)).is_empty());
    let broken = text.replace("base.spit", "missing.spit");
    let errors = diagnose_in(&broken, None, Context::at(&main));
    assert_eq!(errors[0].line, Some(2));
    assert!(errors[0].message.contains("cannot load import"));
}

#[test]
fn an_operation_a_library_declares_in_a_stage_may_be_used_anywhere() {
    let dir = Tree::new("imports", &[]);
    dir.write("base.spit", "stage tools:\n    operation clean(input)\n");
    let main = dir.write(
        "main.spit",
        "source raw [id]\nuse base.spit\nstage prep:\n    result = clean(raw)\n",
    );
    let text = fs::read_to_string(&main).unwrap();
    assert!(diagnose_in(&text, None, Context::at(&main)).is_empty());
}

#[test]
fn a_local_operation_may_not_take_an_imported_name() {
    let dir = Tree::new("imports", &[]);
    dir.write("base.spit", "operation clean(input)\n");
    let main = dir.write(
        "main.spit",
        "source raw [id]\nuse base.spit\nstage prep:\n    operation clean(input)\n    result = clean(raw)\n",
    );
    let text = fs::read_to_string(&main).unwrap();
    let errors = diagnose_in(&text, None, Context::at(&main));
    assert_eq!(errors[0].line, Some(4));
    assert_eq!(
        errors[0].message,
        "duplicate operation `clean`: the `use` on line 2 imports one; operations are global even when declared in a stage, so give this one another name"
    );
}

#[test]
fn an_imported_source_keeps_its_path_rule_under_its_alias() {
    let dir = Tree::new("imports", &[]);
    dir.write(
        "base.spit",
        "path: input/{@product}/{id}.txt\nsource raw [id]\noperation clean(input)\n",
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
    let dir = Tree::new("imports", &[]);
    dir.write("base as draft.spit", "operation clean(input)\n");
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
    let dir = Tree::new("imports", &[]);
    dir.write(
        "base.spit",
        "source raw [id]\noperation clean(input)\ncommand clean: cp {input} {@output}\ncleaned = clean(raw)\n",
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
    assert!(diagnose_in(&text, None, Context::at(path)).is_empty());
}

#[test]
fn an_imported_path_keeps_escaped_braces() {
    let dir = Tree::new("imports", &[]);
    dir.write(
        "base.spit",
        "path: input/{{product}}/{@product}/{id}.txt\nsource raw [id]\n",
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

#[test]
fn an_import_may_not_define_again_what_the_file_defines() {
    let dir = Tree::new("imports", &[]);
    dir.write(
        "base.spit",
        "source raw [id]\npath raw: in/{id}.txt\noperation clean(input)\n\
         command clean: tool {input} {@output}\n",
    );
    for (first, name, kind) in [
        ("source raw [id]", "raw", "product"),
        ("operation clean(input)", "clean", "operation"),
        (
            "command clean: tool {input} {@output}",
            "clean",
            "command for operation",
        ),
        ("path raw: x/{id}.txt", "raw", "path for product"),
    ] {
        let main = dir.write(
            "main.spit",
            &format!("{first}\nuse {name} from base.spit\n"),
        );
        let error = parse_pipeline_at(&fs::read_to_string(&main).unwrap(), &main).unwrap_err();
        assert_eq!(error.line(), 2);
        assert_eq!(
            error.message(),
            format!("import conflicts with {kind} `{name}`")
        );
    }
}

const PHOTOS: &str = "sidecars photo [shot]:\n    path: photos/{shot}\n    source raw : Image .raw\n    source meta : Json .json\n";

const COPY: &str =
    "operation cp(x: Image) -> Image\ncommand cp: cp {x} {@output}\nout = cp(l::raw)\n";

/// `use` brings a `sidecars` group in whole: the group, as `alias::name`,
/// and its members, as the sources `alias::member` with their paths.
#[test]
fn an_import_brings_a_sidecars_group_in_whole() {
    let dir = Tree::new("imports", &[]);
    dir.write("lib.spit", PHOTOS);
    for (use_line, group) in [
        ("use lib.spit as l", "l::photo"),
        ("use photo from lib.spit as l", "l::photo"),
    ] {
        let main = dir.write("main.spit", &format!("{use_line}\n{COPY}"));
        let pipeline = parse_pipeline_at(&fs::read_to_string(&main).unwrap(), &main).unwrap();
        let [imported] = pipeline.sidecar_groups.as_slice() else {
            panic!("one group, found {:?}", pipeline.sidecar_groups);
        };
        assert_eq!(imported.name, group);
        assert_eq!(
            imported.members,
            [
                ("l::raw".to_owned(), ".raw".to_owned()),
                ("l::meta".to_owned(), ".json".to_owned())
            ]
        );
        let paths: Vec<_> = ["l::raw", "l::meta"]
            .map(|name| pipeline.path_template_for(name).unwrap().to_string())
            .into();
        assert_eq!(paths, ["photos/{shot}.raw", "photos/{shot}.json"]);
    }
}

#[test]
fn a_sidecars_member_is_not_imported_alone() {
    let dir = Tree::new("imports", &[]);
    dir.write("lib.spit", PHOTOS);
    let main = dir.write("main.spit", "use raw from lib.spit as l\n");
    let error = parse_pipeline_at(&fs::read_to_string(&main).unwrap(), &main).unwrap_err();
    assert_eq!(error.line(), 1);
    assert_eq!(
        error.message(),
        "`raw` is a member of sidecars group `photo`; import the group, `photo`, to bring its members"
    );
}

/// An imported group keeps what the group is for: a recipe gives its stem by
/// the group's qualified name, and a binding that has some members and not
/// others is reported as it is where the group is written.
#[test]
fn an_imported_sidecars_group_still_reports_incomplete_bindings() {
    let dir = Tree::new("imports", &["d/p/1.raw", "d/p/1.json", "d/p/2.raw"]);
    dir.write(
        "lib.spit",
        "sidecars photo [shot]:\n    source raw : Image .raw\n    source meta : Json .json\n",
    );
    dir.write(
        "top.spit",
        "use photo from lib.spit as l\noperation cp(x: Image, m: Json) -> Image\n\
         command cp: cp {x} {m} {@output}\nout = cp(l::raw, l::meta)\n",
    );
    let recipe = dir.write(
        "top.spitin",
        "pipeline top.spit\nroot d\npath l::photo: p/{shot}\n",
    );
    let output = support::spit(&["inputs", recipe.to_str().unwrap()]);
    let stderr = support::text(&output.stderr);
    assert!(
        stderr.contains("l::photo[shot=2] has .raw but no .json"),
        "{stderr}"
    );
}
