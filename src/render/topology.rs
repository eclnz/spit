//! Connected product/step diagrams before dataset expansion.

use crate::model::Pipeline;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeSet;

/// Render a compiled pipeline as downward flows. Products are bracketed,
/// operations parenthesised, and shared dependencies remain connected rails.
/// The graph is indexed once and walked without recursion; drawing work grows
/// with the diagram's printed area rather than duplicating shared subgraphs.
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
                inputs: call.inputs.iter().map(String::as_str).collect(),
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
    let mut text = String::from("Pipeline\n");
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
    if text.contains('╪') {
        text.push_str("\n╪ = crossing, no connection; ┼ = junction.\n");
    }
    text
}

struct Step<'p> {
    name: &'p str,
    inputs: Vec<&'p str>,
    outputs: &'p [String],
    stage: Option<&'p str>,
}

impl<'p> Step<'p> {
    fn from_invocation(invocation: &'p crate::model::Invocation) -> Self {
        Self {
            name: &invocation.operation,
            inputs: invocation
                .inputs
                .iter()
                .map(|input| input.product.as_str())
                .collect(),
            outputs: &invocation.outputs,
            stage: invocation.stage.as_deref(),
        }
    }
}

fn render_graph(products: &[&str], steps: &[Step<'_>]) -> String {
    let product_count = products.len();
    let nodes = product_count + steps.len();
    let index: FxHashMap<_, _> = products
        .iter()
        .enumerate()
        .map(|(id, &name)| (name, id))
        .collect();
    let mut edges = vec![Vec::new(); nodes];
    let mut parents = vec![Vec::new(); nodes];
    for (step, invocation) in steps.iter().enumerate() {
        let id = product_count + step;
        for &input in &invocation.inputs {
            let product = index[input];
            if edges[product].last() != Some(&id) {
                edges[product].push(id);
                parents[id].push(product);
            }
        }
        for output in invocation.outputs {
            let product = index[output.as_str()];
            edges[id].push(product);
            parents[product].push(id);
        }
    }
    let labels: Vec<_> = products
        .iter()
        .map(|p| format!("[{p}]"))
        .chain(steps.iter().map(|s| format!("({})", s.name)))
        .collect();
    let width = (labels.iter().map(|s| s.chars().count()).max().unwrap_or(0) + 4) | 1;
    let mut layout = Layout {
        columns: Vec::new(),
        free: BTreeSet::new(),
        positions: vec![None; nodes],
        width,
        remaining: edges.iter().map(Vec::len).collect(),
        extent: 0,
        text: String::new(),
    };
    let mut seen = vec![false; nodes];
    let mut stack = Vec::new();
    for root in (0..nodes).filter(|&id| edges[id].is_empty()) {
        stack.push((root, false));
        while let Some((id, finish)) = stack.pop() {
            if finish {
                if id < product_count {
                    layout.product(id, &labels[id]);
                } else {
                    layout.operation(
                        id,
                        &labels[id],
                        steps[id - product_count].stage,
                        &parents[id],
                        &edges[id],
                    );
                }
            } else if !seen[id] {
                seen[id] = true;
                stack.push((id, true));
                stack.extend(parents[id].iter().rev().map(|&parent| (parent, false)));
            }
        }
    }
    layout.text
}

struct Layout {
    /// One rail per live product, shared by every operation that reads it.
    columns: Vec<Option<usize>>,
    free: BTreeSet<usize>,
    extent: usize,
    positions: Vec<Option<usize>>,
    width: usize,
    remaining: Vec<usize>,
    text: String,
}

impl Layout {
    fn allocate(&mut self, node: usize) -> usize {
        self.allocate_from(node, 0)
    }

    fn allocate_from(&mut self, node: usize, minimum: usize) -> usize {
        let column = if let Some(&column) = self.free.range(minimum..).next() {
            self.free.remove(&column);
            column
        } else {
            self.columns.push(None);
            self.columns.len() - 1
        };
        self.extent = self.extent.max(column + 1);
        self.columns[column] = Some(node);
        self.positions[node] = Some(column);
        column
    }

    fn row(&self) -> Vec<char> {
        let extent = self.extent;
        let mut row = vec![' '; extent * self.width];
        for (column, node) in self.columns[..extent].iter().enumerate() {
            if node.is_some() {
                row[column * self.width + self.width / 2] = '│';
            }
        }
        row
    }

    fn write_row(&mut self, row: &[char]) {
        let end = row.iter().rposition(|&c| c != ' ').map_or(0, |i| i + 1);
        self.text.extend(row[..end].iter());
        self.text.push('\n');
    }

    fn label(&mut self, column: usize, label: &str, stage: Option<&str>, incoming: bool) {
        let center = column * self.width + self.width / 2;
        if incoming {
            let mut row = self.row();
            row[center] = '▼';
            self.write_row(&row);
        }
        let mut row = self.row();
        let start = center - label.chars().count() / 2;
        for (offset, character) in label.chars().enumerate() {
            row[start + offset] = character;
        }
        if let Some(stage) = stage {
            let end = row.iter().rposition(|&c| c != ' ').map_or(0, |i| i + 1);
            row.truncate(end);
            row.extend(format!("  stage: {stage}").chars());
        }
        self.write_row(&row);
    }

    fn release(&mut self, node: usize) {
        let column = self.positions[node]
            .take()
            .expect("a displayed node has a rail");
        self.columns[column] = None;
        self.free.insert(column);
        self.shrink_extent();
    }

    fn shrink_extent(&mut self) {
        while self.extent > 0 && self.columns[self.extent - 1].is_none() {
            self.extent -= 1;
        }
    }

    fn product(&mut self, node: usize, label: &str) {
        let incoming = self.positions[node].is_some();
        if !incoming && !self.text.is_empty() {
            let row = self.row();
            self.write_row(&row);
        }
        let column = self.positions[node].unwrap_or_else(|| self.allocate(node));
        self.label(column, label, None, incoming);
        if self.remaining[node] == 0 {
            self.release(node);
        }
    }

    fn operation(
        &mut self,
        node: usize,
        label: &str,
        stage: Option<&str>,
        inputs: &[usize],
        outputs: &[usize],
    ) {
        let mut sources = Vec::with_capacity(inputs.len());
        let mut reusable = None;
        for &input in inputs {
            let column = self.positions[input].expect("an operation's inputs were displayed first");
            self.remaining[input] -= 1;
            let keep = self.remaining[input] != 0;
            sources.push((column, keep));
            if !keep && reusable.is_none_or(|(_, previous)| column < previous) {
                reusable = Some((input, column));
            }
        }
        let boxed = sources.len() > 1;
        let column = if boxed {
            self.route_around_box(&sources);
            let column = sources
                .iter()
                .map(|&(i, _)| i)
                .min()
                .expect("a multi-input box has inputs");
            for (&input, &(old_column, keep)) in inputs.iter().zip(&sources) {
                if !keep {
                    self.positions[input] = None;
                }
                self.columns[old_column] = None;
                if old_column != column {
                    self.free.insert(old_column);
                }
            }
            self.columns[column] = Some(node);
            self.positions[node] = Some(column);
            column
        } else if let Some((input, column)) = reusable {
            self.positions[input] = None;
            self.positions[node] = Some(column);
            self.columns[column] = Some(node);
            column
        } else {
            self.allocate(node)
        };
        if boxed {
            self.box_label(column, label, stage, &sources, !outputs.is_empty());
        } else {
            if !sources.is_empty() {
                self.connect(&sources, &[column]);
            }
            self.label(column, label, stage, !sources.is_empty());
        }
        self.positions[node] = None;
        self.columns[column] = None;
        let mut destinations = Vec::with_capacity(outputs.len());
        for &output in outputs {
            let destination = if self.columns[column].is_none() {
                self.columns[column] = Some(output);
                self.positions[output] = Some(column);
                column
            } else {
                self.allocate(output)
            };
            destinations.push(destination);
        }
        if !destinations.is_empty() {
            self.connect(&[(column, false)], &destinations);
        }
        if self.columns[column].is_none() {
            self.free.insert(column);
        }
        self.shrink_extent();
        self.compact();
    }

    /// Reclaim gaps left by consumed inputs. Without this routing pass, a
    /// product shared by a long chain of joins would drift right at each box.
    fn compact(&mut self) {
        let mut target = 0;
        for column in 0..self.extent {
            let Some(node) = self.columns[column] else {
                continue;
            };
            if column != target {
                self.columns[column] = None;
                self.columns[target] = Some(node);
                self.positions[node] = Some(target);
                self.free.insert(column);
                self.free.remove(&target);
                self.connect(&[(column, false)], &[target]);
                self.shrink_extent();
            }
            target += 1;
        }
    }

    /// Move pass-through lines outside the box. An input needed again
    /// branches here: one arrow enters the box and its other line continues.
    fn route_around_box(&mut self, sources: &[(usize, bool)]) {
        let left = sources
            .iter()
            .map(|&(i, _)| i)
            .min()
            .expect("a box has inputs");
        let right = sources
            .iter()
            .map(|&(i, _)| i)
            .max()
            .expect("a box has inputs");
        let mut input = vec![None; right - left + 1];
        for &(column, keep) in sources {
            input[column - left] = Some(keep);
        }
        for column in left..=right {
            let Some(node) = self.columns[column] else {
                continue;
            };
            let keep = input[column - left];
            if keep == Some(false) {
                continue;
            }
            let destination = self.allocate_from(node, right + 1);
            if keep.is_none() {
                self.columns[column] = None;
                self.free.insert(column);
            }
            self.connect(&[(column, keep.is_some())], &[destination]);
        }
    }

    fn box_label(
        &mut self,
        column: usize,
        label: &str,
        stage: Option<&str>,
        sources: &[(usize, bool)],
        output: bool,
    ) {
        let left = sources
            .iter()
            .map(|&(i, _)| i)
            .min()
            .expect("a box has inputs")
            * self.width
            + self.width / 2
            - 2;
        let right = sources
            .iter()
            .map(|&(i, _)| i)
            .max()
            .expect("a box has inputs")
            * self.width
            + self.width / 2
            + 2;
        let mut row = self.row();
        row.resize(row.len().max(right + 1), ' ');
        for &(source, _) in sources {
            row[source * self.width + self.width / 2] = '▼';
        }
        self.write_row(&row);
        let mut row = self.row();
        row.resize(row.len().max(right + 1), ' ');
        row[left..=right].fill('─');
        row[left] = '┌';
        row[right] = '┐';
        self.write_row(&row);
        let mut row = self.row();
        row.resize(row.len().max(right + 1), ' ');
        row[left..=right].fill(' ');
        row[left] = '│';
        row[right] = '│';
        let title = label
            .strip_prefix('(')
            .and_then(|s| s.strip_suffix(')'))
            .expect("operation labels are parenthesised");
        let start = left + (right - left + 1 - title.chars().count()) / 2;
        for (offset, character) in title.chars().enumerate() {
            row[start + offset] = character;
        }
        if let Some(stage) = stage {
            let end = row.iter().rposition(|&c| c != ' ').map_or(0, |i| i + 1);
            row.truncate(end);
            row.extend(format!("  stage: {stage}").chars());
        }
        self.write_row(&row);
        let mut row = self.row();
        row.resize(row.len().max(right + 1), ' ');
        row[left..=right].fill('─');
        row[left] = '└';
        row[right] = '┘';
        if output {
            row[column * self.width + self.width / 2] = '┬';
        }
        self.write_row(&row);
    }

    fn connect(&mut self, sources: &[(usize, bool)], targets: &[usize]) {
        let left = sources
            .iter()
            .map(|&(i, _)| i)
            .chain(targets.iter().copied())
            .min()
            .expect("a connection has endpoints");
        let right = sources
            .iter()
            .map(|&(i, _)| i)
            .chain(targets.iter().copied())
            .max()
            .expect("a connection has endpoints");
        let mut row = self.row();
        row.resize(row.len().max(right * self.width + self.width / 2 + 1), ' ');
        let left_x = left * self.width + self.width / 2;
        let right_x = right * self.width + self.width / 2;
        for cell in &mut row[left_x..=right_x] {
            *cell = if *cell == '│' { '╪' } else { '─' };
        }
        let mut endpoints = vec![(false, false); right - left + 1];
        for &(column, keep) in sources {
            endpoints[column - left].0 = true;
            endpoints[column - left].1 |= keep;
        }
        for &column in targets {
            endpoints[column - left].1 = true;
        }
        for (offset, &(up, down)) in endpoints.iter().enumerate() {
            if up || down {
                let x = (left + offset) * self.width + self.width / 2;
                row[x] = junction(up, down, x > left_x, x < right_x);
            }
        }
        self.write_row(&row);
    }
}

fn junction(up: bool, down: bool, left: bool, right: bool) -> char {
    match (up, down, left, right) {
        (true, true, true, true) => '┼',
        (true, true, true, false) => '┤',
        (true, true, false, true) => '├',
        (true, false, true, true) => '┴',
        (false, true, true, true) => '┬',
        (true, false, true, false) => '┘',
        (true, false, false, true) => '└',
        (false, true, true, false) => '┐',
        (false, true, false, true) => '┌',
        _ => '│',
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
        assert!(text.lines().all(|line| line.chars().count() < 45));
        assert!(text.contains("[p999]"));
    }

    #[test]
    fn shared_and_unrelated_products_bypass_input_boxes() {
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
        let lines: Vec<_> = text.lines().collect();
        let boxes: Vec<_> = lines
            .windows(2)
            .filter(|pair| pair[1].contains('┌') && pair[1].contains('┐'))
            .collect();
        assert_eq!(boxes.len(), 3);
        for pair in boxes {
            assert_eq!(pair[0].matches('▼').count(), 2);
        }
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
        assert_eq!(render_pipeline_tree(&pipeline), "Pipeline\n  [t1_parcellation]\n          │\n          │               [asl_in_t1]\n          ▼                    ▼\n        ┌────────────────────────┐\n        │    regional_values     │\n        └─┬──────────────────────┘\n          │\n          ▼\n   [regional_asl]\n          │\n          ▼\n  (group_analysis)\n          │\n          ▼\n      [results]\n");
    }

    #[test]
    fn branches_and_joins_stay_connected() {
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
        assert!(text.contains('├'));
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
        assert!(text.contains("(combine)  stage: prep"));
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
        assert_eq!(text.lines().count(), 60_002);
        assert!(text.contains("[p10000]"));
    }
}
