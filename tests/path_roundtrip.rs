//! A path SPIT renders for a binding is a path discovery reads back to that
//! binding. Rendering (binding a DAG's paths) and matching (finding source
//! files) both read a product's compiled path pattern, so this runs both
//! over one table of templates and values.

mod support;

use std::collections::BTreeSet;

use spit::{
    bind_dag, discover_source_files, parse_pipeline_at, resolve, EntityBinding, InputRules,
    SourceInventory, SourceRecord,
};
use support::Tree;

/// One dimension's value in a binding.
type Pair = (&'static str, &'static str);

/// What discovery gives for the paths a case renders.
enum Back {
    /// The bindings the paths were rendered from.
    Same,
    /// A known disagreement: rendering writes `renders`, which discovery
    /// reads as `matches` rather than as the case's bindings.
    Differs {
        renders: &'static [&'static str],
        matches: &'static [&'static [Pair]],
    },
}

/// A source, its path rule, and the bindings rendered with it.
struct Case {
    name: &'static str,
    /// The source as a step reads it: `lib::raw` when imported.
    source: &'static str,
    /// The pipeline's lines that declare the source and its rule, and any
    /// steps beyond the one the test adds.
    pipeline: &'static str,
    /// `lib.spit`, which the pipeline imports, if it does.
    library: Option<&'static str>,
    bindings: &'static [&'static [Pair]],
    back: Back,
}

/// Values at the edge of what a path component keeps: letters, digits and
/// `-` as they are, and every other byte, `%` and `_` among them, escaped.
/// A space and `,[]=#` are left out, as no inventory holds them.
const EDGE_VALUES: &[&[Pair]] = &[
    &[("a", "01")],
    &[("a", "A-b")],
    &[("a", "-")],
    &[("a", "a+b~c")],
    &[("a", "a_b")],
    &[("a", "a.b")],
    &[("a", "a/b")],
    &[("a", "100%")],
    &[("a", "%41")],
    &[("a", "%")],
    &[("a", "é")],
    &[("a", "{a}:b!")],
];

const CASES: &[Case] = &[
    Case {
        name: "a dimension alone in its file name",
        source: "raw",
        pipeline: "source raw [a]\npath raw: raw/{a}.txt\n",
        library: None,
        bindings: EDGE_VALUES,
        back: Back::Same,
    },
    Case {
        name: "a dimension as a directory, after a literal ending in a value character",
        source: "raw",
        pipeline: "source raw [a]\npath raw: raw/sub{a}/data\n",
        library: None,
        bindings: EDGE_VALUES,
        back: Back::Same,
    },
    Case {
        name: "a dimension at the end of the path",
        source: "raw",
        pipeline: "source raw [a]\npath raw: raw/a-{a}\n",
        library: None,
        bindings: EDGE_VALUES,
        back: Back::Same,
    },
    Case {
        name: "a dimension written twice",
        source: "raw",
        pipeline: "source raw [a]\npath raw: raw/{a}/{a}_scan.txt\n",
        library: None,
        bindings: EDGE_VALUES,
        back: Back::Same,
    },
    Case {
        name: "two dimensions apart by a character no value holds",
        source: "raw",
        pipeline: "source raw [a, b]\npath raw: raw/{a}_{b}.txt\n",
        library: None,
        bindings: &[
            &[("a", "a_b"), ("b", "c")],
            &[("a", "x"), ("b", "y_z")],
            &[("a", "%5F"), ("b", "100%")],
            &[("a", "é"), ("b", "-")],
        ],
        back: Back::Same,
    },
    Case {
        name: "two dimensions apart by `-`, which a value may hold",
        source: "raw",
        pipeline: "source raw [a, b]\npath raw: raw/{a}-{b}.txt\n",
        library: None,
        bindings: &[&[("a", "x"), ("b", "y")], &[("a", "x+y"), ("b", "z_w")]],
        back: Back::Same,
    },
    // `-` is kept as it is, so a value holding it gives the same path as a
    // shorter value before the `-`: matching takes the shortest first value.
    // `spit dag` warns of such a value, which `tests/robustness.rs` checks.
    Case {
        name: "two dimensions apart by `-`, the first holding `-`",
        source: "raw",
        pipeline: "source raw [a, b]\npath raw: raw/{a}-{b}.txt\n",
        library: None,
        bindings: &[&[("a", "x-y"), ("b", "z")]],
        back: Back::Differs {
            renders: &["raw/x-y-z.txt"],
            matches: &[&[("a", "x"), ("b", "y-z")]],
        },
    },
    Case {
        name: "each shape, and one of any length beside ones of a fixed length",
        source: "raw",
        pipeline: "source raw [y, d, n, m]\npath raw: raw/{y:year}{n:digits}/{d:date}{m:digits}_{n}.txt\n",
        library: None,
        bindings: &[
            &[("y", "2024"), ("d", "2024-02-29"), ("n", "007"), ("m", "1")],
            &[("y", "1900"), ("d", "2099-12-31"), ("n", "0"), ("m", "120")],
        ],
        back: Back::Same,
    },
    Case {
        name: "a shape on one placeholder of a dimension written twice",
        source: "raw",
        pipeline: "source raw [n]\npath raw: raw/{n:digits}/{n}.txt\n",
        library: None,
        bindings: &[&[("n", "01")], &[("n", "12345")]],
        back: Back::Same,
    },
    Case {
        name: "the product's name",
        source: "raw",
        pipeline: "source raw [a]\npath raw: data/{@product}/{a}.txt\n",
        library: None,
        bindings: &[&[("a", "x")], &[("a", "a_b")]],
        back: Back::Same,
    },
    Case {
        name: "an imported product's name",
        source: "lib::raw",
        pipeline: "use lib.spit as lib\n",
        library: Some("source raw [a]\npath raw: data/{@product}/{a}.txt\n"),
        bindings: &[&[("a", "x")], &[("a", "a.b")]],
        back: Back::Same,
    },
    Case {
        name: "the entities, spelled with a key and value",
        source: "raw",
        pipeline: "default entities: {@key}-{@value} separated \"_\"\nsource raw [sub, ses]\npath raw: raw/{@entities}_scan.txt\n",
        library: None,
        bindings: &[
            &[("sub", "01"), ("ses", "a-b")],
            &[("sub", "A_B"), ("ses", "100%")],
        ],
        back: Back::Same,
    },
    Case {
        name: "the entities, with a label and affixes",
        source: "raw",
        pipeline: "default entities: e_{@key}:{@value}! separated \".\"\ndefault entities sub: subject_id\nsource raw [sub, ses]\npath raw: raw/{@entities}.txt\n",
        library: None,
        bindings: &[
            &[("sub", "A_b/é"), ("ses", "01")],
            &[("sub", "x.y"), ("ses", "a:b!")],
        ],
        back: Back::Same,
    },
    Case {
        name: "a group kept and a group dropped",
        source: "raw",
        pipeline: "source raw [a, b]\npath raw: raw/{a}[/ses-{ses}][/{@stage}][_run{b}].txt\nsource other [ses]\npath other: other/{ses}.txt\nread = copy(other)\npath read: out/read/{ses}\n",
        library: None,
        bindings: &[&[("a", "x"), ("b", "1")], &[("a", "a_b"), ("b", "%")]],
        back: Back::Same,
    },
    Case {
        name: "a source extension",
        source: "raw",
        pipeline: "source raw .nii.gz [a]\npath raw: raw/{a}\n",
        library: None,
        bindings: &[&[("a", "x")], &[("a", "x.nii")]],
        back: Back::Same,
    },
    Case {
        name: "a folder source",
        source: "raw",
        pipeline: "source raw / [a]\npath raw: raw/{a}\n",
        library: None,
        bindings: EDGE_VALUES,
        back: Back::Same,
    },
    // Outputs are matched too, to tell an earlier run's files from files no
    // rule matches: every output path here must be one discovery holds.
    Case {
        name: "outputs in nested stages",
        source: "raw",
        pipeline: "source raw [a]\npath raw: raw/{a}.txt\ndefault path: out/{@stage}/{@product}/{a}.txt\nstage prep:\n    stage inner:\n        staged = copy(raw)\n    folded = copy(staged)\n",
        library: None,
        bindings: EDGE_VALUES,
        back: Back::Same,
    },
];

/// `bindings` as a set, to compare without order.
fn binding_set(bindings: &[&[Pair]]) -> BTreeSet<EntityBinding> {
    bindings
        .iter()
        .map(|pairs| {
            pairs
                .iter()
                .map(|&(dimension, value)| (dimension.to_owned(), value.to_owned()))
                .collect()
        })
        .collect()
}

/// Render `case`'s bindings into files, then discover them, giving the
/// rendered source paths and the bindings discovery found for them.
fn round_trip(case: &Case) -> (BTreeSet<String>, BTreeSet<EntityBinding>) {
    let code = Tree::new("path-roundtrip-code", &[]);
    if let Some(library) = case.library {
        code.write("lib.spit", library);
    }
    let dimensions: Vec<_> = case.bindings[0].iter().map(|(name, _)| *name).collect();
    let text = format!(
        "operation copy(x)\n{}copied = copy({})\npath copied: out/copied/{}\n",
        case.pipeline,
        case.source,
        dimensions
            .iter()
            .map(|dimension| format!("{{{dimension}}}"))
            .collect::<Vec<_>>()
            .join("/"),
    );
    let file = code.write("pipeline.spit", &text);
    let pipeline = parse_pipeline_at(&text, &file)
        .unwrap_or_else(|error| panic!("{}: {error}\n{text}", case.name));
    let inventory = SourceInventory {
        artifacts: binding_set(case.bindings)
            .into_iter()
            .map(|entities| SourceRecord::new(case.source, entities))
            .collect(),
        ..SourceInventory::default()
    };
    let dag = resolve(&pipeline, &inventory)
        .unwrap_or_else(|error| panic!("{}: {error}\n{text}", case.name));
    let bound =
        bind_dag(&pipeline, &dag).unwrap_or_else(|error| panic!("{}: {error}\n{text}", case.name));

    let data = Tree::new("path-roundtrip-data", &[]);
    let mut sources = BTreeSet::new();
    let mut every = BTreeSet::new();
    for job in &bound.jobs {
        for &id in job.inputs.iter().flatten().chain(&job.outputs) {
            let artifact = bound.artifact(id);
            if artifact.product == case.source {
                sources.insert(artifact.path.to_owned());
            }
            if artifact.folder {
                std::fs::create_dir_all(data.path().join(artifact.path)).unwrap();
            } else {
                data.write(artifact.path, "");
            }
            every.insert(artifact.path.to_owned());
        }
    }
    let discovery = discover_source_files(&pipeline, &InputRules::default(), data.path())
        .unwrap_or_else(|error| panic!("{}: {error}", case.name));
    assert!(
        discovery.skipped.is_empty() && discovery.unmatched_files.is_empty(),
        "{}: of the rendered paths {every:?}, discovery skipped {:?} and matched no rule with {:?}",
        case.name,
        discovery.skipped,
        discovery.unmatched_files,
    );
    let found = discovery
        .inventory
        .artifacts
        .into_iter()
        .filter(|record| record.product == case.source)
        .map(|record| record.entities)
        .collect();
    (sources, found)
}

#[test]
fn a_rendered_path_matches_back_to_its_binding() {
    for case in CASES {
        let (rendered, found) = round_trip(case);
        match &case.back {
            Back::Same => assert_eq!(
                found,
                binding_set(case.bindings),
                "{}: rendered {rendered:?}",
                case.name
            ),
            Back::Differs { renders, matches } => {
                assert_eq!(
                    rendered,
                    renders.iter().map(|path| (*path).to_owned()).collect(),
                    "{}",
                    case.name
                );
                assert_eq!(found, binding_set(matches), "{}", case.name);
            }
        }
    }
}
