//! A job's work: what it reads, writes and runs, written as the compact
//! JSON its fingerprint hashes.

use std::ops::Range;

use crate::json::{write_array, write_string, ObjectWriter, Out};

use super::write::write_command;
use super::{BoundCommand, BoundDag};

/// Where a job's commands are in the work its fingerprint hashes.
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

/// Write a job's `commands` into its work. A job with one command or none
/// hashes as one did when a job had a single `command` with its `stdin` and
/// `stdout`, each left out when null, so its fingerprint is unchanged; one
/// with several hashes its `commands` as the document writes them.
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

/// A 64-bit FNV-1a hash of a job's work written as compact JSON, as 16
/// hexadecimal digits: the same on every platform and in every release,
/// unlike the standard library's. The JSON writer's format is part of the
/// `.spitdag` contract; a test pins a job's value.
pub(super) fn fingerprint(work: &str) -> u64 {
    let mut hash = Fnv::new();
    hash.push_str(work);
    hash.0
}

/// `value` as 16 hexadecimal digits.
pub(super) fn write_hex(out: &mut String, value: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for shift in (0..16).rev() {
        out.push(char::from(HEX[(value >> (4 * shift)) as usize & 0xf]));
    }
}

/// FNV-1a over the text written to it, so nothing is kept but the hash.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Out for Fnv {
    fn push_str(&mut self, text: &str) {
        for byte in text.bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}
