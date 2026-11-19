//! Arithmetic as data shared by the pipeline and its bound jobs.

/// A finite decimal, input port, or application of a fixed arithmetic operator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expression {
    /// A finite decimal in JSON number syntax.
    Number(String),
    /// A single-file input port of the operation.
    Input(String),
    /// An operator or function with one operand.
    Unary(UnaryOperator, Box<Expression>),
    /// An operator or function with two operands in evaluation order.
    Binary(BinaryOperator, Box<[Expression; 2]>),
}

/// An arithmetic operator or function that takes one operand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOperator {
    /// Change the sign of a number.
    Negate,
    /// Raise e to a power.
    Exp,
    /// Take the natural logarithm.
    Log,
    /// Take the square root.
    Sqrt,
    /// Take the absolute value.
    Abs,
}

impl UnaryOperator {
    /// The operator's name in a DAG prefix array.
    // Keep in step with OPERATORS in runner/src/spit_bash/calculation.py.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Negate => "neg",
            Self::Exp => "exp",
            Self::Log => "log",
            Self::Sqrt => "sqrt",
            Self::Abs => "abs",
        }
    }
}

/// An arithmetic operator or function that takes two operands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOperator {
    /// Add two numbers.
    Add,
    /// Subtract the second number from the first.
    Subtract,
    /// Multiply two numbers.
    Multiply,
    /// Divide the first number by the second.
    Divide,
    /// Raise the first number to the second power.
    Power,
    /// Take the smaller number.
    Min,
    /// Take the larger number.
    Max,
    /// Round the first number to the given number of decimal places.
    Round,
}

impl BinaryOperator {
    /// The operator's name in a DAG prefix array.
    // Keep in step with OPERATORS in runner/src/spit_bash/calculation.py.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Power => "^",
            Self::Min => "min",
            Self::Max => "max",
            Self::Round => "round",
        }
    }
}

impl Expression {
    /// Every input reference in evaluation order.
    pub(crate) fn inputs(&self) -> impl Iterator<Item = &str> {
        let mut pending = vec![self];
        std::iter::from_fn(move || {
            while let Some(node) = pending.pop() {
                match node {
                    Self::Input(name) => return Some(name.as_str()),
                    Self::Unary(_, operand) => pending.push(operand),
                    Self::Binary(_, operands) => pending.extend(operands.iter().rev()),
                    Self::Number(_) => {}
                }
            }
            None
        })
    }
}
