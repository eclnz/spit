use crate::model::{CheckDef, CheckUse, Pipeline, ReviewDef};

use super::{Module, Selection};

pub(super) fn import_check(
    selected: &mut Selection,
    module: &Module<'_>,
    name: &str,
    alias: Option<&str>,
) {
    let qualified = qualified_name(name, alias);
    if selected.checks.contains(&qualified) {
        return;
    }
    if let Some(check) = module.checks.get(name) {
        selected.checks.insert(qualified.clone());
        selected.pipeline.checks.push(CheckDef {
            name: qualified,
            ..(*check).clone()
        });
    }
}

pub(super) fn qualified_name(name: &str, alias: Option<&str>) -> String {
    alias.map_or_else(|| name.to_owned(), |alias| format!("{alias}::{name}"))
}

pub(super) fn import_review(
    selected: &mut Selection,
    module: &Module<'_>,
    name: &str,
    alias: Option<&str>,
) {
    let qualified = qualified_name(name, alias);
    if module.reviews.contains(name) && selected.reviews.insert(qualified.clone()) {
        selected
            .pipeline
            .reviews
            .push(ReviewDef { name: qualified });
    }
}

pub(super) fn qualify_attached(selected: &mut Pipeline, alias: Option<&str>) {
    if alias.is_none() {
        return;
    }
    let qualify = |uses: &mut Vec<CheckUse>| {
        for used in uses {
            used.check = qualified_name(&used.check, alias);
        }
    };
    let qualify_reviews = |names: &mut Vec<String>| {
        for name in names {
            *name = qualified_name(name, alias);
        }
    };
    for operation in &mut selected.operations {
        for step in &mut operation.steps {
            for (_, used) in &mut step.invocation.checks {
                used.check = qualified_name(&used.check, alias);
            }
            for (_, name) in &mut step.invocation.reviews {
                *name = qualified_name(name, alias);
            }
        }
        for port in &mut operation.inputs {
            qualify(&mut port.checks);
            qualify_reviews(&mut port.reviews);
        }
        for port in &mut operation.outputs {
            qualify(&mut port.checks);
            qualify_reviews(&mut port.reviews);
            qualify_reviews(&mut port.exempt_reviews);
        }
    }
    for product in &mut selected.products {
        qualify(&mut product.checks);
        qualify_reviews(&mut product.reviews);
    }
}
