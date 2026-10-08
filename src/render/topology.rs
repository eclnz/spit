//! A product/step graph before dataset expansion. Each node is expanded once.

use crate::model::Pipeline;
use rustc_hash::FxHashMap;
use std::fmt::Write;

/// Takes a compiled pipeline, whose steps reference declared products.
/// Render sources flowing through steps to products. Joins point to nodes
/// already shown using their stable numbers. Indentation is capped so long
/// pipelines remain linear in output size as well as traversal work.
pub fn render_pipeline_tree(pipeline: &Pipeline) -> String {
    let products = pipeline.products.len();
    let nodes = products + pipeline.invocations.len();
    let index: FxHashMap<_, _> = pipeline
        .products
        .iter()
        .enumerate()
        .map(|(id, product)| (product.name.as_str(), id))
        .collect();
    let mut edges = vec![Vec::new(); nodes];
    let mut incoming = vec![false; nodes];
    for (step, invocation) in pipeline.invocations.iter().enumerate() {
        let id = products + step;
        // A step may bind the same product to several ports. Show one edge.
        for input in &invocation.inputs {
            let product = index[input.product.as_str()];
            if edges[product].last() != Some(&id) {
                edges[product].push(id);
            }
            incoming[id] = true;
        }
        for output in &invocation.outputs {
            let product = index[output.as_str()];
            edges[id].push(product);
            incoming[product] = true;
        }
    }
    let mut text = String::from("Pipeline topology (references use node numbers):\n");
    let mut seen = vec![false; nodes];
    let mut stack = Vec::new();
    // Declaration order makes both roots and siblings deterministic.
    for root in (0..nodes).filter(|&id| !incoming[id]) {
        stack.push((root, 0));
        while let Some((id, depth)) = stack.pop() {
            for _ in 0..depth.min(20) {
                text.push_str("  ");
            }
            if depth > 20 {
                text.push_str("... ");
            }
            if seen[id] {
                writeln!(text, "+-> [{}] (see above)", id + 1).expect("writing to a String");
                continue;
            }
            seen[id] = true;
            if id < products {
                writeln!(text, "+-> [{}] {}", id + 1, pipeline.products[id].name)
                    .expect("writing to a String");
            } else {
                let step = &pipeline.invocations[id - products];
                write!(text, "+-> [{}] ({})", id + 1, step.operation).expect("writing to a String");
                if let Some(stage) = &step.stage {
                    write!(text, " [stage {stage}]").expect("writing to a String");
                }
                text.push('\n');
            }
            stack.extend(edges[id].iter().rev().map(|&child| (child, depth + 1)));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InputBinding, Invocation, ProductDef};
    use crate::types::TypeExpr;

    #[test]
    fn isolated_products_and_inputless_steps_are_roots() {
        let pipeline = Pipeline {
            products: ["unused", "out"]
                .into_iter()
                .map(|name| ProductDef::new(name, TypeExpr::Unknown, [] as [&str; 0]))
                .collect(),
            invocations: vec![Invocation::new("generate", vec![], "out")],
            ..Pipeline::default()
        };
        let text = render_pipeline_tree(&pipeline);
        assert!(text.contains("+-> [1] unused\n+-> [3] (generate)\n  +-> [2] out"));
    }

    #[test]
    fn repeated_input_ports_show_one_edge_and_stage() {
        let pipeline = Pipeline {
            products: ["raw", "out"]
                .into_iter()
                .map(|name| ProductDef::new(name, TypeExpr::Unknown, [] as [&str; 0]))
                .collect(),
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
        assert!(text.contains("(combine) [stage prep]"));
        assert!(!text.contains("see above"));
    }

    #[test]
    fn deep_chains_use_bounded_indentation() {
        let count = 10_000;
        let mut pipeline = Pipeline::default();
        for id in 0..=count {
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
        assert_eq!(text.lines().count(), 2 * count + 2);
        assert!(text.lines().all(|line| line.len() < 90));
        assert!(text.contains("p10000"));
    }

    #[test]
    fn branches_and_joins_expand_once() {
        let pipeline = Pipeline {
            products: ["a", "b", "c", "d"]
                .into_iter()
                .map(|name| ProductDef::new(name, TypeExpr::Unknown, [] as [&str; 0]))
                .collect(),
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
        assert_eq!(text, "Pipeline topology (references use node numbers):\n+-> [1] a\n  +-> [5] (split)\n    +-> [2] b\n      +-> [6] (join)\n        +-> [4] d\n    +-> [3] c\n      +-> [6] (see above)\n");
    }
}
