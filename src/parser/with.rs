//! A `with` line: properties for jobs, which a backend reads and SPIT does
//! not. `with:` sets them for a file or stage, `with operation name:` for
//! one operation's jobs, and `with product name:` for the jobs that make
//! one product.

use crate::model::Prop;

use super::lexical::identifier;
use super::ParseError;

/// What a `with` line sets properties for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WithTarget {
    /// The file, or the stage the line is in.
    Scope,
    Operation(String),
    Product(String),
}

/// Whether `line` is a `with` line rather than a step that makes a product
/// named `with`. Keep in step with `Keyword::split`, which calls it.
pub(super) fn is_with(rest: &str) -> bool {
    if let Some(after) = rest.strip_prefix(':') {
        let first = after.split_whitespace().next();
        return first
            .is_none_or(|word| word.split_once('=').is_some_and(|(key, _)| !key.is_empty()));
    }
    let Some(words) = rest.strip_prefix(' ') else {
        return false;
    };
    let mut words = words.trim_start().splitn(2, char::is_whitespace);
    matches!(words.next(), Some("operation" | "product"))
        && words.next().is_some_and(|tail| tail.contains(':'))
}

/// Read what follows `with`. Keys are lowercase words, and a value is one
/// word or text in double quotes; `key=-` takes the key away.
pub(super) fn parse_with(rest: &str, number: usize) -> Result<(WithTarget, Vec<Prop>), ParseError> {
    let (target, list) = match rest.strip_prefix(':') {
        Some(list) => (WithTarget::Scope, list),
        None => {
            let (head, list) = rest.split_once(':').ok_or_else(|| {
                ParseError::new(number, "expected `with operation name: key=value`")
            })?;
            let mut words = head.split_whitespace();
            let (kind, name) = (words.next(), words.next());
            if words.next().is_some() {
                return Err(ParseError::new(
                    number,
                    "expected one name before the `:` of a `with` line",
                ));
            }
            let name = identifier(name.unwrap_or_default(), number, "name")?.to_owned();
            let target = match kind {
                Some("operation") => WithTarget::Operation(name),
                Some("product") => WithTarget::Product(name),
                _ => unreachable!("`is_with` accepts only `operation` and `product`"),
            };
            (target, list)
        }
    };
    let props = properties(list, number)?;
    if props.is_empty() {
        return Err(ParseError::new(
            number,
            "expected `key=value` after the `:` of a `with` line, such as `with: cpus=4 mem=8G`",
        ));
    }
    Ok((target, props))
}

fn properties(list: &str, number: usize) -> Result<Vec<Prop>, ParseError> {
    let mut props: Vec<Prop> = Vec::new();
    let mut rest = list.trim();
    while !rest.is_empty() {
        let (key, after) = rest.split_once('=').ok_or_else(|| {
            ParseError::new(number, format!("expected `key=value`, found `{rest}`")).at_token(rest)
        })?;
        let valid = key.starts_with(|c: char| c.is_ascii_lowercase())
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !valid {
            return Err(ParseError::new(
                number,
                format!("`{key}` is not a property name; write lowercase letters, digits and `_`, starting with a letter"),
            )
            .at_token(key));
        }
        let (value, remainder, quoted) = match after.strip_prefix('"') {
            Some(quoted) => {
                let (value, remainder) = quoted.split_once('"').ok_or_else(|| {
                    ParseError::new(
                        number,
                        format!("the quoted value of `{key}` has no closing `\"`"),
                    )
                    .at_token(key)
                })?;
                (value, remainder, true)
            }
            None => {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..end], &after[end..], false)
            }
        };
        if value.is_empty() && !quoted {
            return Err(ParseError::new(
                number,
                format!("`{key}=` has no value; write `{key}=-` to take the property away"),
            )
            .at_token(key));
        }
        if !remainder.is_empty() && !remainder.starts_with(char::is_whitespace) {
            return Err(ParseError::new(
                number,
                format!("expected a space after the value of `{key}`"),
            )
            .at_token(key));
        }
        if props.iter().any(|prop| prop.key == key) {
            return Err(ParseError::new(
                number,
                format!("`{key}` is given twice on one `with` line"),
            )
            .at_token(key));
        }
        props.push(Prop {
            key: key.to_owned(),
            value: (quoted || value != "-").then(|| value.to_owned()),
        });
        rest = remainder.trim_start();
    }
    Ok(props)
}

#[cfg(test)]
mod tests {
    use super::{is_with, parse_with, WithTarget};

    #[test]
    fn a_step_that_makes_a_product_named_with_is_not_a_with_line() {
        assert!(is_with(": cpus=4"));
        assert!(is_with(" operation denoise: cpus=4"));
        assert!(is_with(" product clean: mem=8G"));
        assert!(!is_with(" = f(x)"));
        assert!(!is_with(": Image = f(x)"));
        assert!(!is_with(" : Image [sub] = f(x)"));
    }

    #[test]
    fn values_are_words_or_quoted_text_and_a_dash_takes_a_key_away() {
        let (target, props) = parse_with(" operation denoise: cpus=8 queue=\"long jobs\" mem=-", 1)
            .expect("a valid line");
        assert_eq!(target, WithTarget::Operation("denoise".to_owned()));
        let shown: Vec<_> = props
            .iter()
            .map(|prop| (prop.key.as_str(), prop.value.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                ("cpus", Some("8")),
                ("queue", Some("long jobs")),
                ("mem", None)
            ]
        );
    }

    #[test]
    fn mistakes_are_errors() {
        for bad in [
            ": cpus",
            ": Cpus=1",
            ": cpus=",
            ": cpus=1 cpus=2",
            ": q=\"x",
            ":",
        ] {
            assert!(parse_with(bad, 1).is_err(), "{bad}");
        }
    }
}
