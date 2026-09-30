//! The JSON SPIT writes, through serde_json in one format, so a
//! fingerprint, which hashes a job's JSON, never depends on who wrote it.

use std::io;

use serde::Serialize;
use serde_json::ser::{CharEscape, CompactFormatter, Formatter};

/// Write `value` to `writer` as compact JSON.
pub(crate) fn write<W: io::Write>(writer: W, value: &impl Serialize) -> serde_json::Result<()> {
    value.serialize(&mut serde_json::Serializer::with_formatter(
        writer,
        SpitFormatter,
    ))
}

/// `value` as compact JSON.
pub(crate) fn to_string(value: &impl Serialize) -> String {
    let mut out = Vec::new();
    // SPIT's values are strings, numbers, arrays and objects with string
    // keys, which always serialize, and writing into memory cannot fail.
    write(&mut out, value).expect("SPIT's JSON values always serialize");
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

/// serde_json's compact JSON, except that a backspace and a form feed are
/// written as `\u0008` and `\u000c`, as every `.spitdag` has written them.
struct SpitFormatter;

impl Formatter for SpitFormatter {
    fn write_char_escape<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        char_escape: CharEscape,
    ) -> io::Result<()> {
        match char_escape {
            CharEscape::Backspace => writer.write_all(b"\\u0008"),
            CharEscape::FormFeed => writer.write_all(b"\\u000c"),
            other => CompactFormatter.write_char_escape(writer, other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_string;

    #[test]
    fn values_are_written_compactly_with_escapes() {
        #[derive(serde::Serialize)]
        struct Value<'a> {
            text: &'a str,
            items: (usize, Option<usize>),
            empty: [(); 0],
        }
        let value = Value {
            text: "a\"\\\n\u{1}\u{8}\u{c}\u{1f}\u{7f}é",
            items: (1, None),
            empty: [],
        };
        assert_eq!(
            to_string(&value),
            "{\"text\":\"a\\\"\\\\\\n\\u0001\\u0008\\u000c\\u001f\u{7f}é\",\"items\":[1,null],\"empty\":[]}"
        );
    }
}
