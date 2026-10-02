//! The `spit` command line, which `main.rs` runs. It belongs to the binary,
//! not the library, and uses only what `lib.rs` exports.

pub(crate) mod args;
pub(crate) mod commands;
mod load;
pub(crate) mod output;
