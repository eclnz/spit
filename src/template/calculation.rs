#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expression {
    Number(String),
    Input(String),
    Unary(UnaryOperator, Box<Expression>),
    Binary(BinaryOperator, Box<[Expression; 2]>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOperator {
    Negate,
    Exp,
    Log,
    Sqrt,
    Abs,
}

impl UnaryOperator {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    Min,
    Max,
    Round,
}

impl BinaryOperator {
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
