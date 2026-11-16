//! A path rule's text read into its parts, the checks on parts that touch,
//! and the parts written back as text.

use super::path::{PathPart, PathPlaceholder};
use super::shape::Shape;
use super::{parse_template, Part};

/// Read a path template's parts: `[...]` groups, each holding text and
/// placeholders, between text, placeholders and `*`. `[[`, `]]` and `**`
/// are a literal bracket and star, as `{{` and `}}` are literal braces.
pub(super) fn parse_parts(text: &str) -> Result<Vec<PathPart>, String> {
    let mut parts = Vec::new();
    // The text since the last bracket or `*`, with `[[`, `]]` and `**` read.
    let mut segment = String::new();
    // Where the open group's `[` is.
    let mut open = None;
    // The parts of the open group read before its last `*`.
    let mut group = Vec::new();
    // Whether a placeholder is open, whose `*` is its own text.
    let mut braced = false;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        let doubled = characters.peek().map(|&(_, next)| next) == Some(character);
        match character {
            '{' | '}' if doubled && !braced => {
                characters.next();
                segment.push(character);
                segment.push(character);
            }
            '{' => {
                braced = true;
                segment.push(character);
            }
            '}' => {
                braced = false;
                segment.push(character);
            }
            _ if braced => segment.push(character),
            '[' | ']' | '*' if doubled => {
                characters.next();
                segment.push(character);
            }
            '*' => {
                let into = if open.is_some() {
                    &mut group
                } else {
                    &mut parts
                };
                push_segment(into, &std::mem::take(&mut segment))?;
                into.push(PathPart::Wildcard);
            }
            '[' => {
                if open.is_some() {
                    return Err(format!(
                        "`[` inside `[...]` in `{text}`; a group cannot hold another"
                    ));
                }
                push_segment(&mut parts, &std::mem::take(&mut segment))?;
                open = Some(index);
            }
            ']' => {
                let start = open.take().ok_or_else(|| {
                    format!("unmatched `]` in `{text}`; write `]]` for a literal `]`")
                })?;
                let mut group = std::mem::take(&mut group);
                push_segment(&mut group, &std::mem::take(&mut segment))?;
                if !group.iter().any(|part| {
                    matches!(
                        part,
                        PathPart::Placeholder(
                            PathPlaceholder::Dimension(..)
                                | PathPlaceholder::Stage
                                | PathPlaceholder::Entities
                        )
                    )
                }) {
                    return Err(format!(
                        "`{}` names nothing that could be absent; remove the brackets",
                        &text[start..=index]
                    ));
                }
                parts.push(PathPart::Group(group));
            }
            character => segment.push(character),
        }
    }
    if open.is_some() {
        return Err(format!(
            "unclosed `[` in `{text}`; write `[[` for a literal `[`"
        ));
    }
    push_segment(&mut parts, &segment)?;
    if parts.is_empty() {
        parts.push(PathPart::Literal(String::new()));
    }
    Ok(parts)
}

/// The parts of `parts` with each `[...]` group's parts in its place.
pub(super) fn flatten<'a>(parts: &'a [PathPart], out: &mut Vec<&'a PathPart>) {
    for part in parts {
        match part {
            PathPart::Group(group) => flatten(group, out),
            part => out.push(part),
        }
    }
}

/// Fail when a dimension has two shapes, when a fixed dimension is fixed
/// twice or read from a name too, or when two placeholders with nothing
/// between them both take a shape of any length, as `{a:digits}{b:digits}`
/// does: where one ends and the other starts is not written down.
pub(super) fn check_shapes(parts: &[PathPart], text: &str) -> Result<(), String> {
    let mut all = Vec::new();
    flatten(parts, &mut all);
    for (index, part) in all.iter().enumerate() {
        let PathPart::Fixed { dimension, value } = part else {
            continue;
        };
        let again = all.iter().enumerate().any(|(other, part)| match part {
            PathPart::Placeholder(PathPlaceholder::Dimension(name, _)) => name == dimension,
            PathPart::Fixed {
                dimension: name, ..
            } => other != index && name == dimension,
            _ => false,
        });
        if again {
            return Err(format!(
                "`{{{dimension}={value}}}` fixes `{dimension}`, which `{text}` also names elsewhere; a rule reads a dimension from a name or fixes it once"
            ));
        }
    }
    let mut shapes: Vec<(&str, Shape)> = Vec::new();
    for part in &all {
        if let PathPart::Placeholder(PathPlaceholder::Dimension(name, Some(shape))) = part {
            match shapes.iter().find(|(seen, _)| seen == name) {
                Some((_, other)) if other != shape => {
                    return Err(format!(
                        "`{name}` has two shapes, `{other}` and `{shape}`, in `{text}`; give it one"
                    ));
                }
                Some(_) => {}
                None => shapes.push((name, *shape)),
            }
        }
    }
    open_apart(&all, text, false)
}

/// Fail when two placeholders with nothing between them can both be any
/// length, so where one value ends and the next starts is not written down:
/// two shapes of any length, as `{a:digits}{b:digits}`, and with `unshaped`
/// a dimension without a shape too, as `{a}{b}`. `all` is the template's
/// parts with groups flattened away.
pub(super) fn open_apart(all: &[&PathPart], text: &str, unshaped: bool) -> Result<(), String> {
    // A fixed value matches no text, so the parts either side of it touch.
    let all: Vec<&PathPart> = all
        .iter()
        .copied()
        .filter(|part| !matches!(part, PathPart::Fixed { .. }))
        .collect();
    let open = |part: &&PathPart| match part {
        PathPart::Placeholder(PathPlaceholder::Dimension(_, Some(shape))) => {
            shape.fixed_length().is_none()
        }
        PathPart::Placeholder(PathPlaceholder::Dimension(_, None)) => unshaped,
        _ => false,
    };
    // `*` may be any length, so it is apart only from text and from a
    // value of fixed length.
    let beside_star = |part: &&PathPart| match part {
        PathPart::Placeholder(PathPlaceholder::Dimension(_, shape)) => {
            shape.and_then(Shape::fixed_length).is_none()
        }
        PathPart::Wildcard => true,
        _ => false,
    };
    for pair in all.windows(2) {
        let starred = match (pair[0], pair[1]) {
            (PathPart::Wildcard, other) | (other, PathPart::Wildcard) => Some(other),
            _ => None,
        };
        if let Some(other) = starred.filter(|other| beside_star(other)) {
            let (left, right) = match other {
                PathPart::Wildcard => ("*".to_owned(), "*".to_owned()),
                PathPart::Placeholder(placeholder) if matches!(pair[0], PathPart::Wildcard) => {
                    ("*".to_owned(), placeholder.to_string())
                }
                PathPart::Placeholder(placeholder) => (placeholder.to_string(), "*".to_owned()),
                _ => unreachable!("only `*` and dimensions touch a `*` here"),
            };
            return Err(format!(
                "`{left}{right}` in `{text}` has no text between `*` and a value of any length, so a name does not say where one ends; put a `-` or `_` between them"
            ));
        }
        if open(&pair[0]) && open(&pair[1]) {
            let (PathPart::Placeholder(left), PathPart::Placeholder(right)) = (pair[0], pair[1])
            else {
                continue;
            };
            let shaped = |placeholder: &PathPlaceholder| {
                matches!(placeholder, PathPlaceholder::Dimension(_, Some(_)))
            };
            let what = if shaped(left) && shaped(right) {
                "two shapes of any length"
            } else {
                "two values of any length, so a file's name does not say where one ends"
            };
            return Err(format!(
                "`{left}{right}` in `{text}` has no text between {what}; put a `-` or `_` between them"
            ));
        }
    }
    Ok(())
}

/// Add the text and placeholders of `segment`, text between brackets.
fn push_segment(parts: &mut Vec<PathPart>, segment: &str) -> Result<(), String> {
    if segment.is_empty() {
        return Ok(());
    }
    for part in parse_template(segment)? {
        match part {
            Part::Literal(value) => push_literal(parts, &value),
            Part::Placeholder(name) => match name.split_once('=') {
                Some((dimension, value)) if !name.starts_with('@') => {
                    parts.push(fixed(dimension, value)?);
                }
                _ => parts.push(PathPart::Placeholder(PathPlaceholder::parse(name)?)),
            },
        }
    }
    Ok(())
}

/// `{dimension=value}`, checked: a dimension's name and a value an
/// inventory can hold.
fn fixed(dimension: &str, value: &str) -> Result<PathPart, String> {
    let written = format!("{{{dimension}={value}}}");
    if dimension.is_empty() || dimension.contains(':') {
        return Err(format!(
            "`{written}` fixes no dimension; write a dimension's name before `=`, as `{{cohort=ctrl}}`"
        ));
    }
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || ",[]=#".contains(character))
    {
        return Err(format!(
            "`{written}` needs a value with no space or any of `,[]=#`, as `{{cohort=ctrl}}`"
        ));
    }
    Ok(PathPart::Fixed {
        dimension: dimension.to_owned(),
        value: value.to_owned(),
    })
}

/// Add `value` to the text `parts` ends with, so that text is one part.
pub(super) fn push_literal(parts: &mut Vec<PathPart>, value: &str) {
    match parts.last_mut() {
        Some(PathPart::Literal(tail)) => tail.push_str(value),
        _ => parts.push(PathPart::Literal(value.to_owned())),
    }
}

/// The text of `parts`, as a template would write them.
pub(super) fn render(parts: &[PathPart]) -> String {
    let mut text = String::new();
    for part in parts {
        match part {
            PathPart::Literal(value) => push_escaped(&mut text, value),
            PathPart::Placeholder(placeholder) => text.push_str(&placeholder.to_string()),
            PathPart::Group(group) => {
                text.push('[');
                text.push_str(&render(group));
                text.push(']');
            }
            PathPart::Wildcard => text.push('*'),
            PathPart::Fixed { dimension, value } => {
                text.push_str(&format!("{{{dimension}={value}}}"));
            }
        }
    }
    text
}

/// Add `value` to a template's text as a literal, doubling the `{`, `}`,
/// `[`, `]` and `*` that would otherwise open or close a placeholder or a
/// group, or match any text.
pub(crate) fn push_escaped(text: &mut String, value: &str) {
    for character in value.chars() {
        if matches!(character, '{' | '}' | '[' | ']' | '*') {
            text.push(character);
        }
        text.push(character);
    }
}
