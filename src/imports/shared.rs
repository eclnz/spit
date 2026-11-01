//! Names an import brings in beside its operations and sources: the checks
//! and reviews its ports and sources attach.

use crate::model::{CheckDef, CheckUse, Pipeline, ReviewDef};

use super::{Module, Selection};

/// Bring in `module`'s check `name`, once however many imports use it.
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

/// Bring in `module`'s review `name`, once however many imports use it.
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

/// Name the checks and reviews the imported ports and sources use as they
/// are imported.
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
