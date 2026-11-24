mod calculation;
mod collection;
mod command;
mod entities;
mod glob;
mod path;
mod pattern;
mod rule_text;
mod shape;

pub use self::calculation::{BinaryOperator, Expression, UnaryOperator};
pub(crate) use self::collection::CollectionRead;
pub(crate) use self::command::{scalar_text, shell_word, CommandPart, ScalarRead, Stream, SCALAR};
pub use self::command::{CommandError, CommandProblem, CommandTemplate, NamedFile};
pub use self::entities::EntitiesFormat;
pub use self::glob::PathGlob;
pub(crate) use self::path::{
    error, product_text, require_directory, Holder, PathPart, PathPlaceholder,
};
pub use self::path::{PathError, PathProblem, PathTemplate};
pub(crate) use self::pattern::{
    decode_component, encode_component, push_encoded, Gap, PathPattern, RenderError,
};
pub(crate) use self::rule_text::push_escaped;
pub(crate) use self::shape::Shape;

pub(crate) mod builtin {
    pub(crate) const OUTPUT: &str = "@output";
    pub(crate) const CORES: &str = "@cores";
    pub(crate) const HERE: &str = "@here";
    pub(crate) const PRODUCT: &str = "@product";
    pub(crate) const ENTITIES: &str = "@entities";
    pub(crate) const STAGE: &str = "@stage";
    pub(crate) const CHECKED_PATH: &str = "@path";
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Part {
    Literal(String),
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
