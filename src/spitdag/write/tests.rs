use super::*;
use crate::model::{Artifacts, EntityBinding, StepId, Under};
use crate::spitdag::{BoundCommand, BoundStep};
use crate::types::TypeExpr;

fn command(program: &str) -> BoundCommand {
    BoundCommand {
        arguments: vec![vec![ArgPart::Text(program.into())]],
        stdin: None,
        stdout: None,
    }
}

fn step(operation: &str, stage: Option<&str>, inputs: &[&str], outputs: &[&str]) -> BoundStep {
    let names = |ports: &[&str]| ports.iter().map(|&port| port.to_owned()).collect();
    BoundStep {
        operation: operation.to_owned(),
        stage: stage.map(str::to_owned),
        inputs: names(inputs),
        outputs: names(outputs),
        checks: vec![],
        seeds: vec![],
        collection: None,
        origin: None,
        resources: crate::model::Resources::default(),
        tools: crate::model::ToolSet::default(),
        anchored: false,
    }
}

fn bound(steps: Vec<BoundStep>, jobs: impl FnOnce([ArtifactId; 3]) -> Vec<BoundJob>) -> BoundDag {
    let artifact_type = TypeExpr::applied(
        "MRI",
        vec![
            TypeExpr::applied("Pair", vec![TypeExpr::named("T1w"), TypeExpr::Unknown]),
            TypeExpr::named("Diffusion"),
        ],
    );
    bound_of_type(&artifact_type, steps, jobs)
}

fn bound_of_type(
    artifact_type: &TypeExpr,
    steps: Vec<BoundStep>,
    jobs: impl FnOnce([ArtifactId; 3]) -> Vec<BoundJob>,
) -> BoundDag {
    let entities = EntityBinding::from_pairs([("sub", "1"), ("run", "a\"\\\n\u{1}é")]);
    let mut artifacts = Artifacts::default();
    let ids = ["raw", "clean", "mean"].map(|product| {
        let number = artifacts.product(product, artifact_type);
        artifacts.add(number, entities.clone()).unwrap()
    });
    let paths = ["in/1.txt", "out/1.txt", "out/mean.txt"].map(String::from);
    let dimensions = vec![vec!["sub".to_owned(), "run".to_owned()]; 3];
    let under = vec![Under::Root, Under::OutputRoot, Under::OutputRoot];
    BoundDag::new(
        artifacts,
        paths.to_vec(),
        under,
        dimensions,
        vec![false; 3],
        steps,
        jobs(ids),
    )
}

#[test]
fn a_spitdag_is_written_with_its_version_and_escapes() {
    let steps = vec![
        step("clean", Some("prep/denoise"), &["raw"], &["output"]),
        step("mean", None, &["frames"], &["output"]),
    ];
    let mut dag = bound(steps, |[raw, clean, mean]| {
        vec![
            BoundJob {
                id: JobId::new(1),
                step: StepId::new(0),
                inputs: vec![vec![raw]],
                outputs: vec![clean],
                depends_on: vec![],
                missing: Vec::new(),
                commands: vec![BoundCommand {
                    arguments: vec![
                        vec![ArgPart::Text("tool".into())],
                        vec![ArgPart::Text("--in=".into()), ArgPart::Path(raw)],
                    ],
                    stdin: None,
                    stdout: None,
                }],
                verify: vec![vec![vec![ArgPart::Path(raw)]]],
                checks: vec![],
            },
            BoundJob {
                id: JobId::new(2),
                step: StepId::new(1),
                inputs: vec![vec![clean]],
                outputs: vec![mean],
                depends_on: vec![JobId::new(1)],
                missing: Vec::new(),
                commands: vec![],
                verify: vec![],
                checks: vec![],
            },
        ]
    });
    dag.root = Some("/data/study".into());
    dag.output_root = Some("/data/results".into());
    let text = dag.to_json();
    assert!(text.starts_with(&format!(
        "{{\"version\":18,\"generator\":{{\"name\":\"spit\",\"version\":\"{}\"}},\
\"root\":\"/data/study\",\"output_root\":\"/data/results\",\"external_inputs\":[{{\"product\":\"raw\"",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(
        text.contains("\"entities\":{\"sub\":\"1\",\"run\":\"a\\\"\\\\\\n\\u0001é\"}"),
        "{text}"
    );
    assert!(
        text.contains("\"dependents\":[],\"commands\":[],\"verify\":[]"),
        "{text}"
    );
    assert!(
        text.contains("\"path\":\"in/1.txt\",\"under\":\"root\",\"kind\":\"file\"}"),
        "{text}"
    );
    assert!(
        text.contains("\"path\":\"out/1.txt\",\"under\":\"output_root\",\"kind\":\"file\"}"),
        "{text}"
    );
    assert!(
        text.contains("\"executables\":[\"tool\"],\"tools\":[],\"removed\":[],\"left_out\":[],\"reviews\":[],\"pipeline_folder\":null,\"pipeline_files\":[],\"calls\":[],\"jobs\""),
        "{text}"
    );
    assert!(
        text.contains("\"targets\":[{\"product\":\"mean\""),
        "{text}"
    );
    assert_eq!(dag.targets().len(), 1);
    assert!(
        text.contains("\"depends_on\":[],\"dependents\":[2]"),
        "{text}"
    );
    assert!(
        text.contains("\"depends_on\":[1],\"dependents\":[]"),
        "{text}"
    );
}

#[test]
fn a_job_writes_one_or_several_commands_with_their_streams() {
    let steps = vec![step("clean", None, &["raw"], &["output"])];
    let job = |commands: Vec<BoundCommand>| {
        move |[raw, clean, _]: [ArtifactId; 3]| {
            vec![BoundJob {
                id: JobId::new(1),
                step: StepId::new(0),
                inputs: vec![vec![raw]],
                outputs: vec![clean],
                depends_on: vec![],
                missing: Vec::new(),
                commands,
                verify: vec![],
                checks: vec![],
            }]
        }
    };
    let one = bound(steps.clone(), |ids| {
        job(vec![BoundCommand {
            stdout: Some(ids[1]),
            ..command("tool")
        }])(ids)
    });
    let text = one.to_json();
    assert!(
        text.contains(
            "\"commands\":[{\"command\":[[\"tool\"]],\"stdin\":null,\"stdout\":\"out/1.txt\"}]"
        ),
        "{text}"
    );
    let two = bound(steps, job(vec![command("tool"), command("index")])).to_json();
    assert!(
        two.contains("\"commands\":[{\"command\":[[\"tool\"]],\"stdin\":null,\"stdout\":null},{\"command\":[[\"index\"]],\"stdin\":null,\"stdout\":null}],\"verify\":[]"),
        "{two}"
    );
    assert!(
        two.contains("\"executables\":[\"index\",\"tool\"]"),
        "{two}"
    );
}

#[test]
fn a_removal_is_written_with_its_rule_and_why() {
    let mut dag = bound(Vec::new(), |_| Vec::new());
    dag.removed = vec![
        Removal {
            product: Some("bold".into()),
            entities: EntityBinding::from_pairs([("sub", "02"), ("run", "3")]),
            rule: "exclude bold[run=3,sub=02]".into(),
            origin: Some("line 4".into()),
            reason: Some("motion \"spike\"".into()),
            found: None,
        },
        Removal {
            product: None,
            entities: EntityBinding::from_pairs([("sub", "03")]),
            rule: "exclude [sub] where sessions count<2".into(),
            origin: None,
            reason: None,
            found: Some(1),
        },
    ];
    let text = dag.to_json();
    assert!(
        text.contains(
            "\"removed\":[{\"product\":\"bold\",\"entities\":{\"run\":\"3\",\"sub\":\"02\"},\
\"rule\":\"exclude bold[run=3,sub=02]\",\"origin\":\"line 4\",\"reason\":\"motion \\\"spike\\\"\",\
\"found\":null},{\"product\":null,\"entities\":{\"sub\":\"03\"},\"rule\":\"exclude [sub] where sessions count<2\",\
\"origin\":null,\"reason\":null,\"found\":1}]"
        ),
        "{text}"
    );
}

#[test]
fn external_inputs_are_in_the_order_many_inputs_take() {
    let artifact_type = TypeExpr::named("Table");
    let mut artifacts = Artifacts::default();
    let wave = artifacts.product("wave", &artifact_type);
    let fit = artifacts.product("fit", &artifact_type);
    let waves = ["1", "10", "2"].map(|value| {
        artifacts
            .add(wave, EntityBinding::from_pairs([("wave", value)]))
            .unwrap()
    });
    let output = artifacts.add(fit, EntityBinding::from_pairs([])).unwrap();
    let paths = ["w/wave1.csv", "w/wave10.csv", "w/wave2.csv", "fit.json"].map(String::from);
    let job = BoundJob {
        id: JobId::new(1),
        step: StepId::new(0),
        inputs: vec![waves.to_vec()],
        outputs: vec![output],
        depends_on: vec![],
        missing: Vec::new(),
        commands: vec![],
        verify: vec![],
        checks: vec![],
    };
    let dimensions = vec![vec!["wave".to_owned()], vec![]];
    let steps = vec![step("fit", None, &["waves"], &["output"])];
    let under = vec![Under::Root, Under::Root, Under::Root, Under::OutputRoot];
    let dag = BoundDag::new(
        artifacts,
        paths.to_vec(),
        under,
        dimensions,
        vec![false; 2],
        steps,
        vec![job],
    );
    let order: Vec<_> = dag
        .external_inputs()
        .iter()
        .map(|input| input.path)
        .collect();
    assert_eq!(order, ["w/wave1.csv", "w/wave2.csv", "w/wave10.csv"]);
}
