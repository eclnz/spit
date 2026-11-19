//! Arithmetic as data shared by the pipeline and its bound jobs.

/// A finite decimal, input port, or application of a fixed arithmetic operator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expression {
    /// A finite decimal in JSON number syntax.
    Number(String),
    /// A single-file input port of the operation.
    Input(String),
    /// An operator or function followed by its operands in evaluation order.
    Apply(&'static str, Vec<Expression>),
}

impl Expression {
    /// Every input reference in evaluation order.
    pub(crate) fn inputs(&self) -> impl Iterator<Item = &str> {
        let mut pending = vec![self];
        std::iter::from_fn(move || {
            while let Some(node) = pending.pop() {
                match node {
                    Self::Input(name) => return Some(name.as_str()),
                    Self::Apply(_, operands) => pending.extend(operands.iter().rev()),
                    Self::Number(_) => {}
                }
            }
            None
        })
    }
}
