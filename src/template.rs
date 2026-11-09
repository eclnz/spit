//! `{placeholder}` templates shared by path rules and command templates.

/// The built-in placeholders, as their names are written between the
/// braces. A name with `@` is SPIT's, so no port or dimension takes one.
pub(crate) mod builtin {
    /// A command's path for its operation's `-> Type` output.
    pub(crate) const OUTPUT: &str = "@output";
    /// A command's count of the cores its job uses.
    pub(crate) const CORES: &str = "@cores";
    /// A command's folder of the file declaring its operation.
    pub(crate) const HERE: &str = "@here";
    /// A path's product name.
    pub(crate) const PRODUCT: &str = "@product";
    /// A path's every dimension, as `default entities:` spells them.
    pub(crate) const ENTITIES: &str = "@entities";
    /// A path's stage, the one whose block holds the step.
    pub(crate) const STAGE: &str = "@stage";
    /// A check's command's path of the artifact it tests.
    pub(crate) const CHECKED_PATH: &str = "@path";
}

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
