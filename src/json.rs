//! The JSON SPIT writes: a value built as a tree, then written in one place,
//! so no other module escapes strings or places commas.

use std::fmt;

/// A JSON value. Objects keep their fields in the order given.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Number(usize),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// An object with `fields` in order.
    pub(crate) fn object<'a>(fields: impl IntoIterator<Item = (&'a str, Json)>) -> Self {
        Self::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    pub(crate) fn array(items: impl IntoIterator<Item = Json>) -> Self {
        Self::Array(items.into_iter().collect())
    }

    pub(crate) fn string(text: impl Into<String>) -> Self {
        Self::String(text.into())
    }

    /// A number, or `null` when there is none.
    pub(crate) fn number_or_null(value: Option<usize>) -> Self {
        value.map_or(Self::Null, Self::Number)
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Number(number) => write!(f, "{number}"),
            Self::String(text) => write_string(f, text),
            Self::Array(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    fmt::Display::fmt(item, f)?;
                }
                f.write_str("]")
            }
            Self::Object(fields) => {
                write_object(f, fields.iter().map(|(key, value)| (key.as_str(), value)))
            }
        }
    }
}

/// An object of borrowed fields, written exactly as the same fields in a
/// [`Json::Object`], without copying them into one.
pub(crate) struct ObjectRef<'a>(pub(crate) &'a [(&'a str, &'a Json)]);

impl fmt::Display for ObjectRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_object(f, self.0.iter().map(|&(key, value)| (key, value)))
    }
}

fn write_object<'a>(
    f: &mut fmt::Formatter<'_>,
    fields: impl Iterator<Item = (&'a str, &'a Json)>,
) -> fmt::Result {
    f.write_str("{")?;
    for (index, (key, value)) in fields.enumerate() {
        if index > 0 {
            f.write_str(",")?;
        }
        write_string(f, key)?;
        f.write_str(":")?;
        fmt::Display::fmt(value, f)?;
    }
    f.write_str("}")
}

/// `text` as a JSON string. Runs that need no escape are written whole.
fn write_string(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_str("\"")?;
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
        f.write_str(&text[plain..index])?;
        if escape.is_empty() {
            write!(f, "\\u{:04x}", u32::from(character))?;
        } else {
            f.write_str(escape)?;
        }
        plain = index + character.len_utf8();
    }
    f.write_str(&text[plain..])?;
    f.write_str("\"")
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
