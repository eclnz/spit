//! `{placeholder}` templates shared by path rules and command templates.

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Part {
    Literal(String),
    /// A `{name}` placeholder; `{{` and `}}` are literal braces.
    Placeholder(String),
}

pub(crate) fn parse_template(template: &str) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = template.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '{' => {
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') if name.is_empty() => {
                            return Err(format!("empty placeholder `{{}}` in `{template}`"))
                        }
                        Some('}') => break,
                        Some('{') => {
                            return Err(format!("nested `{{` in placeholder in `{template}`"))
                        }
                        Some(value) => name.push(value),
                        None => return Err(format!("unclosed `{{` in `{template}`")),
                    }
                }
                parts.push(Part::Placeholder(name));
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '}' => return Err(format!("unmatched `}}` in `{template}`")),
            value => literal.push(value),
        }
    }
    if !literal.is_empty() || parts.is_empty() {
        parts.push(Part::Literal(literal));
    }
    Ok(parts)
}
