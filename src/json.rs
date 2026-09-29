//! The JSON SPIT writes: a value built as a tree, then written in one place,
//! so no other module escapes strings or places commas.

use std::borrow::Cow;
use std::fmt;

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
    pub(crate) fn write_to<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        match self {
            Self::Null => out.write_str("null"),
            Self::Number(number) => write!(out, "{number}"),
            Self::String(text) => write_string(out, text),
            Self::Array(items) => write_array(out, items, |out, item| item.write_to(out)),
            Self::Object(fields) => {
                write_object(out, fields.iter().map(|(key, value)| (key.as_ref(), value)))
            }
        }
    }
}

impl fmt::Display for Json<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_to(f)
    }
}

/// An object of borrowed fields, written exactly as the same fields in a
/// [`Json::Object`], without copying them into one.
pub(crate) struct ObjectRef<'a, 'b>(pub(crate) &'a [(&'a str, &'a Json<'b>)]);

impl ObjectRef<'_, '_> {
    pub(crate) fn write_to<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        write_object(out, self.0.iter().map(|&(key, value)| (key, value)))
    }
}

impl fmt::Display for ObjectRef<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_to(f)
    }
}

fn write_object<'a, 'b: 'a, W: fmt::Write>(
    out: &mut W,
    fields: impl Iterator<Item = (&'a str, &'a Json<'b>)>,
) -> fmt::Result {
    let mut object = ObjectWriter::start(out)?;
    for (key, value) in fields {
        object.field(key, value)?;
    }
    object.finish()
}

/// Writes an object one field at a time, placing its commas, for a
/// document too large to build as one value first.
pub(crate) struct ObjectWriter<'w, W> {
    out: &'w mut W,
    empty: bool,
}

impl<'w, W: fmt::Write> ObjectWriter<'w, W> {
    pub(crate) fn start(out: &'w mut W) -> Result<Self, fmt::Error> {
        out.write_char('{')?;
        Ok(Self { out, empty: true })
    }

    pub(crate) fn field(&mut self, key: &str, value: &Json<'_>) -> fmt::Result {
        self.field_with(key, |out| value.write_to(out))
    }

    /// A field whose value `write` writes itself.
    pub(crate) fn field_with(
        &mut self,
        key: &str,
        write: impl FnOnce(&mut W) -> fmt::Result,
    ) -> fmt::Result {
        if !self.empty {
            self.out.write_char(',')?;
        }
        self.empty = false;
        write_string(self.out, key)?;
        self.out.write_char(':')?;
        write(self.out)
    }

    pub(crate) fn finish(self) -> fmt::Result {
        self.out.write_char('}')
    }
}

/// An array of `items`, each written by `write`, without building it first.
pub(crate) fn write_array<W: fmt::Write, T>(
    out: &mut W,
    items: impl IntoIterator<Item = T>,
    mut write: impl FnMut(&mut W, T) -> fmt::Result,
) -> fmt::Result {
    out.write_char('[')?;
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write(out, item)?;
    }
    out.write_char(']')
}

/// `text` as a JSON string. Runs that need no escape are written whole.
fn write_string<W: fmt::Write>(out: &mut W, text: &str) -> fmt::Result {
    out.write_char('"')?;
    let mut plain = 0;
    for (index, character) in text.char_indices() {
        let escape = match character {
            '"' => "\\\"",
            '\\' => "\\\\",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            c if c < ' ' => "",
            _ => continue,
        };
        out.write_str(&text[plain..index])?;
        if escape.is_empty() {
            write!(out, "\\u{:04x}", u32::from(character))?;
        } else {
            out.write_str(escape)?;
        }
        plain = index + character.len_utf8();
    }
    out.write_str(&text[plain..])?;
    out.write_char('"')
}

#[cfg(test)]
mod tests {
    use super::Json;

    #[test]
    fn values_are_written_compactly_with_escapes() {
        let value = Json::object([
            ("text", Json::string("a\"\\\n\u{1}é")),
            ("items", Json::array([Json::Number(1), Json::Null])),
            ("empty", Json::object([])),
        ]);
        assert_eq!(
            value.to_string(),
            "{\"text\":\"a\\\"\\\\\\n\\u0001é\",\"items\":[1,null],\"empty\":{}}"
        );
    }
}
