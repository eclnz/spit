use std::collections::BTreeSet;

use crate::check::{step_checks, CheckIndex, When};
use crate::model::{CheckKind, Pipeline, PipelineIndex};
use crate::review::{port_reviews, ReviewIndex};

pub fn render_gate_attachments(pipeline: &Pipeline) -> Vec<String> {
    if !crate::review::has_reviews(pipeline)
        && !pipeline
            .checks
            .iter()
            .any(|check| check.kind == CheckKind::Screen)
    {
        return Vec::new();
    }
    let index = PipelineIndex::new(pipeline);
    let reviews = ReviewIndex::new(&index);
    let checks = CheckIndex::new(&index);
    let mut gates = BTreeSet::new();
    for product in &pipeline.products {
        for review in reviews.of(&product.name) {
            gates.insert(format!(
                "review `{review}` gates readers of `{}`",
                product.name
            ));
        }
        for screen in product
            .checks
            .iter()
            .filter(|used| used.kind == CheckKind::Screen)
        {
            gates.insert(format!(
                "screen `{screen}` gates readers of `{}`",
                product.name
            ));
        }
    }
    for invocation in &pipeline.invocations {
        let Some(operation) = index.operation(&invocation.operation) else {
            continue;
        };
        let call = invocation.outputs.join(", ");
        for (port, input) in operation.inputs.iter().enumerate() {
            for review in port_reviews(operation, invocation, port) {
                gates.insert(format!(
                    "review `{review}` gates input `{}` of `{call}`",
                    input.name
                ));
            }
        }
        for screen in step_checks(&index, &checks, operation, Some(invocation))
            .into_iter()
            .filter(|check| check.check.kind == CheckKind::Screen)
        {
            let label = screen.written;
            match screen.when {
                When::Before if !screen.product_scope => {
                    let port = &operation.inputs[screen.port].name;
                    gates.insert(format!("screen `{label}` gates input `{port}` of `{call}`"));
                }
                When::After => {
                    let output = &invocation.outputs[screen.port];
                    gates.insert(format!("screen `{label}` gates readers of `{output}`"));
                }
                When::Before => {}
            }
        }
    }
    gates.into_iter().collect()
}
