//! A job's commands, written as the JSON the document holds.

use std::ops::Range;

use crate::json::{write_array, write_string, ObjectWriter};

use super::write::write_command;
use super::{BoundCommand, BoundDag};

/// Where a job's commands are in the work buffer.
pub(super) enum WorkCommands {
    /// `"command":null`, for a job with none.
    None,
    /// `"command"`, the one command's arguments, with `"stdin"` and
    /// `"stdout"` after it when they are not null.
    One {
        command: Range<usize>,
        stdin: Option<Range<usize>>,
        stdout: Option<Range<usize>>,
    },
    /// `"commands"`, every command as the document writes it.
    Several(Range<usize>),
}

/// Write a job's `commands` into the work buffer.
pub(super) fn write_work_commands(
    object: &mut ObjectWriter<'_, String>,
    dag: &BoundDag,
    commands: &[BoundCommand],
) -> WorkCommands {
    match commands {
        [] => {
            object.field("command", |out| out.push_str("null"));
            WorkCommands::None
        }
        [only] => WorkCommands::One {
            command: object.field_at("command", |out| write_command(out, dag, &only.arguments)),
            stdin: only
                .stdin
                .map(|id| object.field_at("stdin", |out| write_string(out, dag.path(id)))),
            stdout: only
                .stdout
                .map(|id| object.field_at("stdout", |out| write_string(out, dag.path(id)))),
        },
        several => WorkCommands::Several(object.field_at("commands", |out| {
            write_array(out, several, |out, command| {
                let mut item = ObjectWriter::start(out);
                item.field("command", |out| write_command(out, dag, &command.arguments));
                for (name, file) in [("stdin", command.stdin), ("stdout", command.stdout)] {
                    item.field(name, |out| match file {
                        Some(id) => write_string(out, dag.path(id)),
                        None => out.push_str("null"),
                    });
                }
                item.finish();
            });
        })),
    }
}
