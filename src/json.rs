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
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Self::Object(fields) => {
                f.write_str("{")?;
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    write_string(f, key)?;
                    write!(f, ":{value}")?;
                }
                f.write_str("}")
            }
        }
    }
}

fn write_string(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_str("\"")?;
    for character in text.chars() {
        match character {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if c < ' ' => write!(f, "\\u{:04x}", u32::from(c))?,
            c => write!(f, "{c}")?,
        }
    }
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
