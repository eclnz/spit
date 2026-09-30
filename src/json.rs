//! The JSON SPIT writes, in one place, so no other module escapes strings or
//! places commas. Small documents are built as a [`Json`] value first; the
//! `.spitdag`, which can be large, is written piece by piece.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

/// Where JSON text goes: a `String`, or a hasher that keeps only a hash.
/// Writing cannot fail.
pub(crate) trait Out {
    fn push_str(&mut self, text: &str);

    fn push(&mut self, character: char) {
        self.push_str(character.encode_utf8(&mut [0; 4]));
    }

    /// Make room for at least `additional` more bytes, where that helps.
    fn reserve(&mut self, _additional: usize) {}
}

impl Out for String {
    fn push_str(&mut self, text: &str) {
        String::push_str(self, text);
    }

    fn push(&mut self, character: char) {
        String::push(self, character);
    }

    fn reserve(&mut self, additional: usize) {
        String::reserve(self, additional);
    }
}

/// A JSON value, borrowing its text where it can. Objects keep their fields
/// in the order given.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json<'a> {
    Null,
    Number(usize),
    String(Cow<'a, str>),
    Array(Vec<Json<'a>>),
    Object(Vec<(Cow<'a, str>, Json<'a>)>),
}

impl<'a> Json<'a> {
    /// An object with `fields` in order.
    pub(crate) fn object(fields: impl IntoIterator<Item = (&'a str, Json<'a>)>) -> Self {
        Self::Object(
            fields
                .into_iter()
                .map(|(key, value)| (Cow::Borrowed(key), value))
                .collect(),
        )
    }

    pub(crate) fn array(items: impl IntoIterator<Item = Json<'a>>) -> Self {
        Self::Array(items.into_iter().collect())
    }

    pub(crate) fn string(text: impl Into<Cow<'a, str>>) -> Self {
        Self::String(text.into())
    }

    /// A number, or `null` when there is none.
    pub(crate) fn number_or_null(value: Option<usize>) -> Self {
        value.map_or(Self::Null, Self::Number)
    }

    /// Write this value compactly to `out`.
    pub(crate) fn write_to(&self, out: &mut impl Out) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Number(number) => write_number(out, *number),
            Self::String(text) => write_string(out, text),
            Self::Array(items) => write_array(out, items, |out, item| item.write_to(out)),
            Self::Object(fields) => {
                let mut object = ObjectWriter::start(out);
                for (key, value) in fields {
                    object.field(key, |out| value.write_to(out));
                }
                object.finish();
            }
        }
    }
}

impl fmt::Display for Json<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = String::new();
        self.write_to(&mut text);
        f.write_str(&text)
    }
}

/// Writes an object one field at a time, placing its commas.
pub(crate) struct ObjectWriter<'w, O: Out> {
    out: &'w mut O,
    empty: bool,
}

impl<'w, O: Out> ObjectWriter<'w, O> {
    pub(crate) fn start(out: &'w mut O) -> Self {
        out.push('{');
        Self { out, empty: true }
    }

    /// A field whose value `write` writes.
    pub(crate) fn field(&mut self, key: &str, write: impl FnOnce(&mut O)) {
        self.key(key);
        write(self.out);
    }

    /// A field whose value is already JSON text.
    pub(crate) fn raw(&mut self, key: &str, value: &str) {
        self.key(key);
        self.out.push_str(value);
    }

    pub(crate) fn string(&mut self, key: &str, value: &str) {
        self.key(key);
        write_string(self.out, value);
    }

    fn key(&mut self, key: &str) {
        if !self.empty {
            self.out.push(',');
        }
        self.empty = false;
        write_string(self.out, key);
        self.out.push(':');
    }

    pub(crate) fn finish(self) {
        self.out.push('}');
    }
}

impl ObjectWriter<'_, String> {
    /// A field, as [`ObjectWriter::field`], returning where its value was
    /// written.
    pub(crate) fn field_at(&mut self, key: &str, write: impl FnOnce(&mut String)) -> Range<usize> {
        self.key(key);
        let start = self.out.len();
        write(self.out);
        start..self.out.len()
    }
}

/// An array of `items`, each written by `write`, without building it first.
pub(crate) fn write_array<O: Out, T>(
    out: &mut O,
    items: impl IntoIterator<Item = T>,
    mut write: impl FnMut(&mut O, T),
) {
    out.push('[');
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write(out, item);
    }
    out.push(']');
}

pub(crate) fn write_number(out: &mut impl Out, number: usize) {
    let mut digits = [0; 20];
    let mut start = digits.len();
    let mut rest = number;
    loop {
        start -= 1;
        // The remainder is a single digit.
        digits[start] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    for &digit in &digits[start..] {
        out.push(char::from(digit));
    }
}

/// Whether `text` has a byte a JSON string must escape: a control
/// character, `"` or `\`. Most text has none, so eight bytes are checked at
/// once.
fn needs_escape(text: &str) -> bool {
    const ONES: u64 = 0x0101_0101_0101_0101;
    // Whether any byte of `word` is zero.
    let has_zero = |word: u64| word.wrapping_sub(ONES) & !word & (ONES << 7) != 0;
    let mut chunks = text.as_bytes().chunks_exact(8);
    for chunk in &mut chunks {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(chunk);
        let word = u64::from_le_bytes(bytes);
        // A control character has none of its top three bits set.
        if has_zero(word & (ONES * 0xe0))
            || has_zero(word ^ (ONES * u64::from(b'"')))
            || has_zero(word ^ (ONES * u64::from(b'\\')))
        {
            return true;
        }
    }
    chunks
        .remainder()
        .iter()
        .any(|&byte| byte < b' ' || byte == b'"' || byte == b'\\')
}

/// `text` as a JSON string. Runs that need no escape are written whole.
pub(crate) fn write_string(out: &mut impl Out, text: &str) {
    out.reserve(text.len() + 2);
    out.push('"');
    if !needs_escape(text) {
        out.push_str(text);
        return out.push('"');
    }
    let mut plain = 0;
    for (index, byte) in text.bytes().enumerate() {
        let escape = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            byte if byte < b' ' => "",
            _ => continue,
        };
        out.push_str(&text[plain..index]);
        if escape.is_empty() {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            out.push_str("\\u00");
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0xf)]));
        } else {
            out.push_str(escape);
        }
        plain = index + 1;
    }
    out.push_str(&text[plain..]);
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::{needs_escape, write_number, Json};

    #[test]
    fn values_are_written_compactly_with_escapes() {
        let value = Json::object([
            ("text", Json::string("a\"\\\n\u{1}\u{1f}é")),
            ("items", Json::array([Json::Number(1), Json::Null])),
            ("empty", Json::object([])),
        ]);
        assert_eq!(
            value.to_string(),
            "{\"text\":\"a\\\"\\\\\\n\\u0001\\u001fé\",\"items\":[1,null],\"empty\":{}}"
        );
    }

    #[test]
    fn escapes_are_found_anywhere_in_long_text() {
        for escape in ['"', '\\', '\n', '\u{0}', '\u{1f}'] {
            for at in 0..20 {
                let mut text: String = "sub-01/ses-02/x".repeat(2)[..19].to_owned();
                text.insert(at, escape);
                assert!(needs_escape(&text), "{text:?}");
            }
        }
        for plain in ["", "a", "sub-01/ses-02/dwi_run-01.nii.gz", "é ~ \u{7f} ünï"] {
            assert!(!needs_escape(plain), "{plain:?}");
        }
    }

    #[test]
    fn numbers_are_written_in_decimal() {
        for number in [0, 7, 10, 305, usize::MAX] {
            let mut text = String::new();
            write_number(&mut text, number);
            assert_eq!(text, number.to_string());
        }
    }
}
