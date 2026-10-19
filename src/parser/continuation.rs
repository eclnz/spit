//! Physical lines joined only while an operation signature has open parentheses.

use std::borrow::Cow;

use super::keyword::Keyword;
use super::layout::Layout;

/// Keep in step with recover_document in src/diagnostics/recovery.rs, which
/// blanks all physical lines of a failed logical declaration.
///
/// Keep the ordinary single-line path borrowed. Multiline declarations retain
/// newlines and indentation so token addresses can resolve to physical places.
pub(crate) fn operation_lines(text: &str) -> impl Iterator<Item = (usize, Cow<'_, str>)> {
    let mut lines = Layout::new(text).lines.into_iter().enumerate().peekable();
    std::iter::from_fn(move || {
        let (number, first) = lines.next()?;
        if first.keyword() != Some(Keyword::Operation) {
            return Some((number, Cow::Borrowed(first.text)));
        }
        let mut depth = balance(first.uncommented);
        if depth <= 0 {
            return Some((number, Cow::Borrowed(first.text)));
        }
        let mut joined = first.uncommented.to_owned();
        while depth > 0 {
            let Some((_, next)) = lines.peek() else {
                break;
            };
            // A declaration or a body step is never part of a signature, even
            // when a missing closer leaves the parentheses open.
            if !next.code.is_empty()
                && (next.keyword().is_some()
                    || next.code.contains('=')
                    || (next.indent <= first.indent && !next.code.starts_with(')')))
            {
                break;
            }
            joined.push('\n');
            joined.push_str(next.uncommented);
            depth += balance(next.uncommented);
            lines.next();
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
