//! Collection arguments keep their members separate until paths are rendered.

use std::str::CharIndices;
use std::sync::Arc;

use super::CommandError;

/// A many input rendered as one joined argument or as a temporary list file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CollectionRead {
    /// A port, optionally followed by the dimension whose values are joined.
    pub(crate) subject: String,
    /// `None` writes a list file; `Some` joins with this literal separator.
    pub(crate) separator: Option<Arc<str>>,
}

impl CollectionRead {
    /// Parse the arguments of `@join` or `@list`, without their parentheses.
    pub(crate) fn parse(inside: &str, list: bool) -> Result<Self, CommandError> {
        let name = if list { "@list" } else { "@join" };
        let focus = format!("{name}({inside})");
        let fail = |message: &str| CommandError::new(format!("`{focus}`: {message}")).focus(&focus);
        let (subject, separator) = inside
            .split_once(',')
            .map_or((inside, None), |(a, b)| (a, Some(b.trim())));
        let subject = subject.trim();
        let names: Vec<_> = subject.split('.').collect();
        if names.len() > if list { 1 } else { 2 }
            || names.iter().any(|name| {
                name.is_empty()
                    || !name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            })
        {
            return Err(fail(
                "name a many input port, optionally its dimension for @join",
            ));
        }
        let separator = if list {
            if separator.is_some() {
                return Err(fail("@list takes only an input port"));
            }
            None
        } else {
            let text = separator.unwrap_or("\",\"");
            let Some(text) = text.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
                return Err(fail("write the separator as a double-quoted string"));
            };
            let mut decoded = String::new();
            let mut chars = text.chars();
            while let Some(c) = chars.next() {
                decoded.push(match c {
                    '"' => return Err(fail("escape a quote in the separator with a backslash")),
                    '\\' => match chars.next() {
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        Some('"') => '"',
                        Some('\\') => '\\',
                        _ => {
                            return Err(fail("separator escapes are \\n, \\r, \\t, \\\" and \\\\"))
                        }
                    },
                    c => c,
                });
            }
            if decoded.contains('\0') {
                return Err(fail("a command argument cannot contain NUL"));
            }
            Some(Arc::from(decoded))
        };
        Ok(Self {
            subject: subject.to_owned(),
            separator,
        })
    }
}

/// Find a function's closing parenthesis without treating a quoted `)` as it.
pub(crate) fn function_end(
    text: &str,
    open: usize,
    chars: &mut CharIndices<'_>,
) -> Result<usize, CommandError> {
    let mut quoted = false;
    let mut escaped = false;
    for (at, c) in chars.by_ref() {
        if at < open {
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ')' if !quoted => return Ok(at),
            _ => {}
        }
    }
    Err(
        CommandError::new("collection function is never closed with `)`")
            .focus(&text[open.saturating_sub(6)..]),
    )
}
