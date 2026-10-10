//! The data every step shares: entity bindings, artifacts and their table,
//! what a pipeline declares, a dataset's inputs, and a resolved DAG. Each
//! submodule holds one of these, and this module re-exports them all, so
//! the rest of the crate names them as `crate::model::...`.

mod artifacts;
mod dag;
mod definitions;
mod entities;
mod inputs;
mod pipeline;
mod props;

pub use artifacts::*;
pub use dag::*;
pub use definitions::*;
pub use entities::*;
pub use inputs::*;
pub use pipeline::*;
pub use props::*;

fn owned_strings(values: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect()
}
