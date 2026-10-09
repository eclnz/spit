//! Physical lines joined only while an operation signature has open parentheses.

use std::borrow::Cow;

use super::keyword::Keyword;
use super::lexical::strip_comment;

/// Keep in step with recover_document in src/diagnostics/recovery.rs, which
/// blanks all physical lines of a failed logical declaration.
///
/// Keep the ordinary single-line path borrowed. Multiline declarations retain
/// newlines and indentation so token addresses can resolve to physical places.
pub(crate) fn operation_lines(text: &str) -> impl Iterator<Item = (usize, Cow<'_, str>)> {
    let mut lines = text.lines().enumerate().peekable();
    std::iter::from_fn(move || {
        let (number, first) = lines.next()?;
        let content = strip_comment(first);
        if !matches!(
            Keyword::split(content.trim()),
            Some((Keyword::Operation, _))
        ) {
            return Some((number, Cow::Borrowed(first)));
        }
        let mut depth = balance(content);
        if depth <= 0 {
            return Some((number, Cow::Borrowed(first)));
        }
        let indent = first.len() - first.trim_start().len();
        let mut joined = content.to_owned();
        while depth > 0 {
            let Some(&(_, next)) = lines.peek() else {
                break;
            };
            let content = strip_comment(next);
            let trimmed = content.trim();
            let next_indent = next.len() - next.trim_start().len();
            // A declaration or a body step is never part of a signature, even
            // when a missing closer leaves the parentheses open.
            if !trimmed.is_empty()
                && (Keyword::split(trimmed).is_some()
                    || trimmed.contains('=')
                    || (next_indent <= indent && !trimmed.starts_with(')')))
            {
                break;
            }
            lines.next();
            joined.push('\n');
            joined.push_str(content);
            depth += balance(content);
        }
        Some((number, Cow::Owned(joined)))
    })
}

fn balance(text: &str) -> isize {
    text.bytes().fold(0, |depth, byte| match byte {
        b'(' => depth + 1,
        b')' => depth - 1,
        _ => depth,
    })
}
