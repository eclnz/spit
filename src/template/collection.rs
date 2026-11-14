//! `@join(port)` and `@list(port)`: a `many` input given to a tool as one
//! argument holding every path, or as a file listing them.

use std::fmt;
use std::str::CharIndices;
use std::sync::Arc;

use super::CommandError;

/// The words that open a [`CollectionRead`], up to their `(`.
pub(crate) const JOIN_OPEN: &str = "@join(";
pub(crate) const LIST_OPEN: &str = "@list(";

/// A `@join(...)` or `@list(...)` in a command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CollectionRead {
    /// The `many` input port, or for `@join`, the port and a dimension, as
    /// `xs.run`.
    pub(crate) subject: String,
    /// `@join`'s separator, `,` unless given; `None` for `@list`.
    pub(crate) separator: Option<Arc<str>>,
}

impl fmt::Display for CollectionRead {
    /// The read as a pipeline writes it, with a separator other than `,`:
    /// `@join(xs.run, "; ")`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.separator.as_deref() {
            None => write!(f, "{LIST_OPEN}{})", self.subject),
            Some(",") => write!(f, "{JOIN_OPEN}{})", self.subject),
            Some(separator) => write!(
                f,
                "{JOIN_OPEN}{}, \"{}\")",
                self.subject,
                separator.escape_debug()
            ),
        }
    }
}

impl CollectionRead {
    /// The read `inside` the parentheses gives: of `@list` when `list`,
    /// else of `@join`.
    pub(crate) fn parse(inside: &str, list: bool) -> Result<Self, CommandError> {
        let open = if list { LIST_OPEN } else { JOIN_OPEN };
        let written = format!("{open}{inside})");
        let fail =
            |message: &str| CommandError::new(format!("`{written}`: {message}")).focus(&written);
        let (subject, separator) = match inside.split_once(',') {
            Some((subject, separator)) => (subject.trim(), Some(separator.trim())),
            None => (inside.trim(), None),
        };
        let names: Vec<&str> = subject.split('.').collect();
        let identifier = |name: &&str| {
            name.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if names.len() > if list { 1 } else { 2 } || !names.iter().all(identifier) {
            return Err(fail(if list {
                "name a `many` input port, as in `@list(images)`"
            } else {
                "name a `many` input port, or a port and a dimension, as in `@join(images)` or `@join(images.run)`"
            }));
        }
        let separator = match (list, separator) {
            (true, None) => None,
            (true, Some(_)) => return Err(fail("`@list` takes only a `many` input port")),
            (false, None) => Some(Arc::from(",")),
            (false, Some(text)) => Some(Arc::from(
                separator_text(text).map_err(fail)?,
            )),
        };
        Ok(Self {
            subject: subject.to_owned(),
            separator,
        })
    }
}

/// The separator a `@join` writes as `text`, a double-quoted string with
/// `\n`, `\r`, `\t`, `\"` and `\\` escapes.
fn separator_text(text: &str) -> Result<String, &'static str> {
    let quoted = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or("write the separator in double quotes, as in `\"; \"`")?;
    let mut decoded = String::new();
    let mut characters = quoted.chars();
    while let Some(character) = characters.next() {
        decoded.push(match character {
            '"' => return Err("a `\"` in the separator is written `\\\"`"),
            '\\' => match characters.next() {
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                Some('"') => '"',
                Some('\\') => '\\',
                _ => {
                    return Err(
                        "the separator's escapes are `\\n`, `\\r`, `\\t`, `\\\"` and `\\\\`",
                    )
                }
            },
            '\0' => return Err("a separator cannot hold a NUL"),
            character => character,
        });
    }
    Ok(decoded)
}

/// The index of the `)` that closes a function whose `(` is just before
/// `open` in `text`, moving `chars` past it. A `)` in a double-quoted
/// separator does not close it.
pub(crate) fn function_end(
    text: &str,
    open: usize,
    chars: &mut CharIndices<'_>,
) -> Result<usize, CommandError> {
    let mut quoted = false;
    let mut escaped = false;
    for (at, character) in chars.by_ref() {
        if at < open {
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ')' if !quoted => return Ok(at),
            _ => {}
        }
    }
    let name = &text[open - JOIN_OPEN.len()..open - 1];
    Err(CommandError::new(format!("`{name}(` is never closed with `)`")).focus(name))
}

#[cfg(test)]
mod tests {
    use super::CollectionRead;

    #[test]
    fn a_join_has_a_separator_and_a_list_none() {
        let join = CollectionRead::parse("xs.run, \"; \\t\"", false).expect("a valid join");
        assert_eq!(
            (join.subject.as_str(), join.separator.as_deref()),
            ("xs.run", Some("; \t"))
        );
        assert_eq!(join.to_string(), "@join(xs.run, \"; \\t\")");
        let comma = CollectionRead::parse("xs", false).expect("a valid join");
        assert_eq!(comma.separator.as_deref(), Some(","));
        assert_eq!(
            CollectionRead::parse("xs, \"\"", false)
                .unwrap()
                .separator
                .as_deref(),
            Some("")
        );
        let list = CollectionRead::parse(" xs ", true).expect("a valid list");
        assert_eq!((list.subject.as_str(), list.separator), ("xs", None));
    }

    #[test]
    fn mistakes_say_what_to_write() {
        for (inside, list, message) in [
            ("xs.run", true, "as in `@list(images)`"),
            ("xs, \";\"", true, "takes only a `many` input port"),
            ("my-xs", false, "name a `many` input port"),
            ("xs.run.n", false, "name a `many` input port"),
            ("", false, "name a `many` input port"),
            ("xs, ;", false, "double quotes"),
            ("xs, \"a\"b\"", false, "is written"),
            ("xs, \"\\q\"", false, "escapes are"),
        ] {
            let error = CollectionRead::parse(inside, list).expect_err(inside);
            assert!(error.to_string().contains(message), "{inside}: {error}");
        }
    }
}
