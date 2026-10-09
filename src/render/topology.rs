//! Connected product/step diagrams before dataset expansion.

use crate::model::Pipeline;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;
use std::fmt::Write;

/// Render a compiled pipeline as a stage overview and local diagrams.
/// Product names connect stages; each reusable body is drawn once.
/// Dependencies are indexed once and ordered with an iterative worklist.
pub fn render_pipeline_tree(pipeline: &Pipeline) -> String {
    let mut roots = vec![None; pipeline.calls.len()];
    let mut path = Vec::new();
    for id in 0..pipeline.calls.len() {
        let mut at = id;
        while roots[at].is_none() {
            path.push(at);
            if let Some(parent) = pipeline.calls[at].parent {
                at = parent.index();
            } else {
                roots[at] = Some(at);
                break;
            }
        }
        let root = roots[at].expect("a call has an outermost ancestor");
        for child in path.drain(..) {
            roots[child] = Some(root);
        }
    }
    let mut shown = vec![false; pipeline.calls.len()];
    let mut steps = Vec::new();
    for invocation in &pipeline.invocations {
        if let Some(origin) = &invocation.origin {
            let root = roots[origin.call.index()].expect("every call's root was indexed");
            if std::mem::replace(&mut shown[root], true) {
                continue;
            }
            let call = &pipeline.calls[root];
            steps.push(Step {
                name: &call.operation,
                inputs: unique_inputs(call.inputs.iter().map(String::as_str)),
                outputs: &call.outputs,
                stage: invocation.stage.as_deref(),
            });
        } else {
            steps.push(Step::from_invocation(invocation));
        }
    }
    let produced: FxHashSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|s| s.outputs.iter().map(String::as_str))
        .collect();
    let visible: FxHashSet<_> = steps
        .iter()
        .flat_map(|s| {
            s.inputs
                .iter()
                .copied()
                .chain(s.outputs.iter().map(String::as_str))
        })
        .collect();
    let products: Vec<_> = pipeline
        .products
        .iter()
        .map(|p| p.name.as_str())
        .filter(|name| !produced.contains(name) || visible.contains(name))
        .collect();
    let mut text = if steps.iter().any(|step| step.stage.is_some()) {
        String::new()
    } else {
        String::from("Pipeline\n")
    };
    text.push_str(&render_graph(&products, &steps));
    // A reusable body is drawn once, even when many calls use it. Nested
    // components are discovered iteratively and retain their own boundaries.
    let operations: FxHashMap<_, _> = pipeline
        .operations
        .iter()
        .map(|op| (op.name.as_str(), op))
        .collect();
    let mut queued = FxHashSet::default();
    let mut components = Vec::new();
    for step in &steps {
        if queued.insert(step.name) {
            components.push(step.name);
        }
    }
    let mut at = 0;
    while at < components.len() {
        let name = components[at];
        at += 1;
        let Some(operation) = operations.get(name) else {
            continue;
        };
        if operation.steps.is_empty() {
            continue;
        }
        let mut products: Vec<_> = operation
            .inputs
            .iter()
            .map(|port| port.name.as_str())
            .collect();
        let mut known: FxHashSet<_> = products.iter().copied().collect();
        let steps: Vec<_> = operation
            .steps
            .iter()
            .map(|body| Step::from_invocation(&body.invocation))
            .collect();
        for step in &steps {
            for output in step.outputs {
                if known.insert(output.as_str()) {
                    products.push(output.as_str());
                }
            }
            if queued.insert(step.name) {
                components.push(step.name);
            }
        }
        text.push_str("\nComponent: ");
        text.push_str(name);
        text.push('\n');
        text.push_str(&render_graph(&products, &steps));
    }

    text
}

pub(super) struct Step<'p> {
    pub(super) name: &'p str,
    pub(super) inputs: Vec<&'p str>,
    pub(super) outputs: &'p [String],
    stage: Option<&'p str>,
}

impl<'p> Step<'p> {
    fn from_invocation(invocation: &'p crate::model::Invocation) -> Self {
        Self {
            name: &invocation.operation,
            inputs: unique_inputs(invocation.inputs.iter().map(|input| input.product.as_str())),
            outputs: &invocation.outputs,
            stage: invocation.stage.as_deref(),
        }
    }
}

fn unique_inputs<'p>(inputs: impl Iterator<Item = &'p str>) -> Vec<&'p str> {
    let mut seen = FxHashSet::default();
    inputs.filter(|name| seen.insert(*name)).collect()
}

/// Order dependencies once, using ids and a worklist, without recursive walks.
fn ordered_steps<'a, 'p>(steps: &'a [Step<'p>]) -> Vec<&'a Step<'p>> {
    let producers: FxHashMap<_, _> = steps
        .iter()
        .enumerate()
        .flat_map(|(id, step)| step.outputs.iter().map(move |name| (name.as_str(), id)))
        .collect();
    let parents: Vec<Vec<usize>> = steps
        .iter()
        .map(|step| {
            step.inputs
                .iter()
                .filter_map(|input| producers.get(input).copied())
                .collect()
        })
        .collect();
    let mut seen = vec![false; steps.len()];
    let mut stack = Vec::new();
    let mut ordered = Vec::with_capacity(steps.len());
    for root in 0..steps.len() {
        stack.push((root, false));
        while let Some((id, finish)) = stack.pop() {
            if finish {
                ordered.push(&steps[id]);
            } else if !seen[id] {
                seen[id] = true;
                stack.push((id, true));
                stack.extend(parents[id].iter().rev().map(|&parent| (parent, false)));
            }
        }
    }
    ordered
}

fn overview(steps: &[&Step<'_>], text: &mut String) {
    let mut groups = Vec::new();
    let mut index = FxHashMap::default();
    let mut owners = Vec::new();
    let mut products = FxHashMap::default();
    for step in steps {
        let name = step.stage.map_or("pipeline", |stage| {
            stage.split('/').next().expect("a stage has a name")
        });
        let id = *index.entry(name).or_insert_with(|| {
            groups.push(name);
            groups.len() - 1
        });
        owners.push(id);
        for output in step.outputs {
            products.insert(output.as_str(), id);
        }
    }
    if groups.len() < 2 {
        return;
    }
    let mut edges = BTreeMap::<_, Vec<&str>>::new();
    let mut seen = FxHashSet::default();
    for (step, &to) in steps.iter().zip(&owners) {
        for &input in &step.inputs {
            if let Some(&from) = products.get(input) {
                if from != to && seen.insert((from, to, input)) {
                    edges.entry((from, to)).or_default().push(input);
                }
            }
        }
    }
    text.push_str("Pipeline overview\n");
    writeln!(text, "Stages: {}", groups.join(", ")).expect("writing to a String");
    for ((from, to), products) in edges {
        writeln!(
            text,
            "({}) ──[{}]──> ({})",
            groups[from],
            products.join(", "),
            groups[to]
        )
        .expect("writing to a String");
    }
    text.push_str(
        "\nMatching product names connect stages below; ╪ marks a crossing, not a join.\n\n",
    );
}

fn render_graph(products: &[&str], steps: &[Step<'_>]) -> String {
    let ordered = ordered_steps(steps);
    let mut text = String::new();
    overview(&ordered, &mut text);
    // Index each exact stage once; product rails stay inside its boundary.
    let mut names = Vec::new();
    let mut index = FxHashMap::default();
    let mut groups: Vec<Vec<&Step<'_>>> = Vec::new();
    for step in ordered {
        let id = *index.entry(step.stage).or_insert_with(|| {
            names.push(step.stage);
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[id].push(step);
    }
    for (name, group) in names.iter().zip(&groups) {
        if let Some(name) = name {
            writeln!(text, "Stage: {name}").expect("writing to a String");
        } else if names.len() > 1 {
            text.push_str("Outside stages\n");
        }
        if group.windows(2).all(|pair| {
            pair[1].inputs.len() == 1
                && pair[0].outputs.len() == 1
                && pair[1].inputs[0] == pair[0].outputs[0]
        }) {
            panel(group, &mut text);
        } else {
            super::topology_connected::render(group, &mut text);
        }
        text.push('\n');
    }
    let used: FxHashSet<_> = steps
        .iter()
        .flat_map(|s| {
            s.inputs
                .iter()
                .copied()
                .chain(s.outputs.iter().map(String::as_str))
        })
        .collect();
    for product in products {
        if !used.contains(product) {
            writeln!(text, "Unused source: [{product}]").expect("writing to a String");
        }
    }
    text
}

fn row_width(names: &[&str]) -> usize {
    names
        .iter()
        .map(|name| name.chars().count() + 2)
        .sum::<usize>()
        + names.len().saturating_sub(1) * 3
}

fn product_row(names: &[&str], width: usize, text: &mut String) -> Vec<usize> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut row = vec![' '; width];
    let mut at = (width - row_width(names)) / 2;
    let mut centers = Vec::with_capacity(names.len());
    for name in names {
        let label = format!("[{name}]");
        let len = label.chars().count();
        centers.push(at + len / 2);
        for character in label.chars() {
            row[at] = character;
            at += 1;
        }
        at += 3;
    }
    write_row(&row, text);
    centers
}

pub(super) fn write_row(row: &[char], text: &mut String) {
    let end = row.iter().rposition(|&c| c != ' ').map_or(0, |i| i + 1);
    text.extend(row[..end].iter());
    text.push('\n');
}

fn stems(points: &[usize], width: usize, glyph: char, text: &mut String) {
    let mut row = vec![' '; width];
    for &point in points {
        row[point] = glyph;
    }
    write_row(&row, text);
}

fn panel(steps: &[&Step<'_>], text: &mut String) {
    let width = (steps
        .iter()
        .map(|step| {
            row_width(&step.inputs)
                .max(row_width(
                    &step.outputs.iter().map(String::as_str).collect::<Vec<_>>(),
                ))
                .max(step.name.chars().count() + 2)
        })
        .max()
        .unwrap_or(0)
        + 4)
        | 1;
    let center = width / 2;
    let mut inputs = product_row(&steps[0].inputs, width, text);
    for step in steps {
        if inputs.len() > 1 {
            stems(&inputs, width, '│', text);
            stems(&inputs, width, '▼', text);
            let mut row = vec!['─'; width];
            row[0] = '┌';
            row[width - 1] = '┐';
            write_row(&row, text);
            let mut row = vec![' '; width];
            row[0] = '│';
            row[width - 1] = '│';
            let at = (width - step.name.chars().count()) / 2;
            for (offset, c) in step.name.chars().enumerate() {
                row[at + offset] = c;
            }
            write_row(&row, text);
            let mut row = vec!['─'; width];
            row[0] = '└';
            row[width - 1] = '┘';
            if !step.outputs.is_empty() {
                row[center] = '┬';
            }
            write_row(&row, text);
        } else {
            if !inputs.is_empty() {
                stems(&[center], width, '│', text);
                stems(&[center], width, '▼', text);
            }
            let label = format!("({})", step.name);
            writeln!(
                text,
                "{}{}",
                " ".repeat((width - label.chars().count()) / 2),
                label
            )
            .expect("writing to a String");
        }
        let outputs: Vec<_> = step.outputs.iter().map(String::as_str).collect();
        if outputs.is_empty() {
            inputs.clear();
            continue;
        }
        // All output branching occurs after the operation. Each named output
        // receives its own arrow before the next operation in this chain.
        let mut at = (width - row_width(&outputs)) / 2;
        let points: Vec<_> = outputs
            .iter()
            .map(|name| {
                let len = name.chars().count() + 2;
                let point = at + len / 2;
                at += len + 3;
                point
            })
            .collect();
        if points.len() > 1 {
            stems(&[center], width, '│', text);
            let left = points[0].min(center);
            let right = points[points.len() - 1].max(center);
            let mut row = vec![' '; width];
            row[left..=right].fill('─');
            row[center] = '┴';
            for &point in &points {
                row[point] = if point == center {
                    '┼'
                } else if point == left {
                    '┌'
                } else if point == right {
                    '┐'
                } else {
                    '┬'
                };
            }
            write_row(&row, text);
        } else {
            stems(&points, width, '│', text);
        }
        stems(&points, width, '▼', text);
        inputs = product_row(&outputs, width, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InputBinding, Invocation, ProductDef};
    use crate::types::TypeExpr;

    fn products(names: &[&str]) -> Vec<ProductDef> {
        names
            .iter()
            .map(|&name| ProductDef::new(name, TypeExpr::Unknown, [] as [&str; 0]))
            .collect()
    }

    #[test]
    fn dependencies_precede_readers_even_when_declared_later() {
        let pipeline = Pipeline {
            products: products(&["raw", "a", "b"]),
            invocations: vec![
                Invocation::new("second", vec![InputBinding::product("a")], "b"),
                Invocation::new("first", vec![InputBinding::product("raw")], "a"),
            ],
            ..Pipeline::default()
        };
        let text = render_pipeline_tree(&pipeline);
        assert!(text.find("(first)").unwrap() < text.find("(second)").unwrap());
    }

    #[test]
    fn stage_overview_keeps_named_dependencies() {
        let pipeline = Pipeline {
            products: products(&["raw", "q", "r", "s"]),
            invocations: vec![
                Invocation {
                    stage: Some("prep/import".into()),
                    ..Invocation::new("first", vec![InputBinding::product("raw")], "q")
                },
                Invocation {
                    stage: Some("analysis".into()),
                    ..Invocation::new("second", vec![InputBinding::product("q")], "r")
                },
                Invocation {
                    stage: Some("prep/final".into()),
                    ..Invocation::new("third", vec![InputBinding::product("r")], "s")
                },
            ],
            ..Pipeline::default()
        };
        let text = render_pipeline_tree(&pipeline);
        assert!(text.contains("(prep) ──[q]──> (analysis)"));
        assert!(text.contains("(analysis) ──[r]──> (prep)"));
        for stage in ["prep/import", "analysis", "prep/final"] {
            assert!(text.contains(&format!("Stage: {stage}")));
        }
    }

    #[test]
    fn repeated_joins_with_a_shared_product_stay_narrow() {
        let mut pipeline = Pipeline {
            products: products(&["raw", "seed"]),
            ..Pipeline::default()
        };
        let mut previous = "seed".to_owned();
        for id in 0..1000 {
            let output = format!("p{id}");
            pipeline
                .products
                .push(ProductDef::new(&output, TypeExpr::Unknown, [] as [&str; 0]));
            pipeline.invocations.push(Invocation::new(
                "mix",
                vec![
                    InputBinding::product("raw"),
                    InputBinding::product(previous),
                ],
                &output,
            ));
            previous = output;
        }
        let text = render_pipeline_tree(&pipeline);
        assert!(text.lines().all(|line| line.chars().count() <= 120));
        assert!(text.contains("[p999]"));
    }

    #[test]
    fn shared_products_have_one_connected_rail() {
        let pipeline = Pipeline {
            products: products(&["raw", "a", "b", "c", "x", "y", "out"]),
            invocations: vec![
                Invocation::with_outputs(
                    "split",
                    vec![InputBinding::product("raw")],
                    ["a", "b", "c"],
                ),
                Invocation::new(
                    "mix",
                    vec![InputBinding::product("a"), InputBinding::product("c")],
                    "x",
                ),
                Invocation::new(
                    "mix",
                    vec![InputBinding::product("a"), InputBinding::product("b")],
                    "y",
                ),
                Invocation::new(
                    "finish",
                    vec![InputBinding::product("x"), InputBinding::product("y")],
                    "out",
                ),
            ],
            ..Pipeline::default()
        };
        let text = render_pipeline_tree(&pipeline);
        assert!(text.contains('╪'));
        assert_eq!(text.matches("[a]").count(), 1);
        assert_eq!(text.matches("[b]").count(), 1);
        assert!(text.contains("[out]"));
        assert_eq!(text.matches('▼').count(), 7);
    }

    #[test]
    fn three_inputs_have_three_separate_arrows() {
        let pipeline = Pipeline {
            products: products(&["a", "b", "c", "out"]),
            invocations: vec![Invocation::new(
                "combine",
                vec![
                    InputBinding::product("a"),
                    InputBinding::product("b"),
                    InputBinding::product("c"),
                ],
                "out",
            )],
            ..Pipeline::default()
        };
        let text = render_pipeline_tree(&pipeline);
        let lines: Vec<_> = text.lines().collect();
        let pair = lines
            .windows(2)
            .find(|pair| pair[1].contains('┌'))
            .expect("one input box");
        assert_eq!(pair[0].matches('▼').count(), 3);
        assert!(!pair[0].contains('─'));
    }

    #[test]
    fn two_inputs_join_into_a_vertical_chain() {
        let pipeline = Pipeline {
            products: products(&["t1_parcellation", "asl_in_t1", "regional_asl", "results"]),
            invocations: vec![
                Invocation::new(
                    "regional_values",
                    vec![
                        InputBinding::product("t1_parcellation"),
                        InputBinding::product("asl_in_t1"),
                    ],
                    "regional_asl",
                ),
                Invocation::new(
                    "group_analysis",
                    vec![InputBinding::product("regional_asl")],
                    "results",
                ),
            ],
            ..Pipeline::default()
        };
        assert_eq!(render_pipeline_tree(&pipeline), "Pipeline\n  [t1_parcellation]   [asl_in_t1]\n          │                │\n          ▼                ▼\n┌─────────────────────────────────┐\n│         regional_values         │\n└────────────────┬────────────────┘\n                 │\n                 ▼\n          [regional_asl]\n                 │\n                 ▼\n         (group_analysis)\n                 │\n                 ▼\n             [results]\n\n");
    }

    #[test]
    fn branches_and_joins_show_named_products() {
        let pipeline = Pipeline {
            products: products(&["a", "b", "c", "d"]),
            invocations: vec![
                Invocation::with_outputs("split", vec![InputBinding::product("a")], ["b", "c"]),
                Invocation::new(
                    "join",
                    vec![InputBinding::product("b"), InputBinding::product("c")],
                    "d",
                ),
            ],
            ..Pipeline::default()
        };
        let text = render_pipeline_tree(&pipeline);
        assert_eq!(text.matches("join").count(), 1);
        assert_eq!(text.matches("[b]").count(), 1);
        assert_eq!(text.matches("[c]").count(), 1);
        assert!(text.contains('┘'));
        assert!(!text.contains("see above"));
        assert!(text.contains("[d]"));
    }

    #[test]
    fn repeated_inputs_and_stages() {
        let pipeline = Pipeline {
            products: products(&["raw", "out"]),
            invocations: vec![Invocation {
                stage: Some("prep".into()),
                ..Invocation::new(
                    "combine",
                    vec![InputBinding::product("raw"), InputBinding::product("raw")],
                    "out",
                )
            }],
            ..Pipeline::default()
        };
        let text = render_pipeline_tree(&pipeline);
        assert!(text.contains("(combine)"));
        assert!(text.contains("Stage: prep"));
        assert!(!text.contains('─'));
    }

    #[test]
    fn isolated_products_and_inputless_steps() {
        let pipeline = Pipeline {
            products: products(&["unused", "out"]),
            invocations: vec![Invocation::new("generate", vec![], "out")],
            ..Pipeline::default()
        };
        let text = render_pipeline_tree(&pipeline);
        assert!(text.contains("[unused]"));
        assert!(text.contains("(generate)"));
        assert!(text.contains("[out]"));
    }

    #[test]
    fn long_chains_stay_narrow() {
        let mut pipeline = Pipeline::default();
        for id in 0..=10_000 {
            pipeline.products.push(ProductDef::new(
                format!("p{id}"),
                TypeExpr::Unknown,
                [] as [&str; 0],
            ));
            if id > 0 {
                pipeline.invocations.push(Invocation::new(
                    "next",
                    vec![InputBinding::product(format!("p{}", id - 1))],
                    format!("p{id}"),
                ));
            }
        }
        let text = render_pipeline_tree(&pipeline);
        assert!(text.lines().all(|line| line.chars().count() < 20));
        assert!(text.lines().count() < 70_010);
        assert!(text.contains("[p10000]"));
    }
}
