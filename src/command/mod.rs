//! The checks of command templates against their operations and the steps
//! that call them.

mod validate;

pub(crate) use self::validate::{collect_commands, facet, slot, Facet, Slot};
