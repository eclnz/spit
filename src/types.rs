//! Symbolic, domain-agnostic pipeline types and local unification.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum TypeExpr {
    Named(String),
    Applied {
        constructor: String,
        args: Vec<TypeExpr>,
    },
    Variable(String),
    Unknown,
}

impl TypeExpr {
    pub fn named(name: &str) -> Self {
        Self::Named(name.to_owned())
    }

    pub fn applied(constructor: &str, args: Vec<Self>) -> Self {
        Self::Applied {
            constructor: constructor.to_owned(),
            args,
        }
    }

    pub fn variable(name: &str) -> Self {
        Self::Variable(name.to_owned())
    }

    pub fn is_valid(&self) -> bool {
        match self {
            Self::Named(name) | Self::Variable(name) => !name.is_empty(),
            Self::Applied { constructor, args } => {
                !constructor.is_empty() && !args.is_empty() && args.iter().all(Self::is_valid)
            }
            Self::Unknown => true,
        }
    }

    pub fn has_variables(&self) -> bool {
        match self {
            Self::Variable(_) => true,
            Self::Applied { args, .. } => args.iter().any(Self::has_variables),
            _ => false,
        }
    }

    /// Unresolved signature variables cannot escape their operation invocation.
    /// Keep the known structure while making unknown parameters explicit.
    pub fn erase_variables(&self) -> Self {
        match self {
            Self::Variable(_) => Self::Unknown,
            Self::Applied { constructor, args } => Self::Applied {
                constructor: constructor.clone(),
                args: args.iter().map(Self::erase_variables).collect(),
            },
            _ => self.clone(),
        }
    }

    /// The names of the type variables this expression mentions.
    pub fn variables(&self) -> BTreeSet<String> {
        match self {
            Self::Variable(name) => BTreeSet::from([name.clone()]),
            Self::Applied { args, .. } => args.iter().flat_map(Self::variables).collect(),
            _ => BTreeSet::new(),
        }
    }

    fn contains_variable(&self, name: &str) -> bool {
        match self {
            Self::Variable(value) => value == name,
            Self::Applied { args, .. } => args.iter().any(|arg| arg.contains_variable(name)),
            _ => false,
        }
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => f.write_str(name),
            Self::Variable(name) if name.len() == 1 => f.write_str(name),
            Self::Variable(name) => write!(f, "${name}"),
            Self::Applied { constructor, args } => {
                let args = args.iter().map(ToString::to_string).collect::<Vec<_>>();
                write!(f, "{constructor}<{}>", args.join(","))
            }
            Self::Unknown => f.write_str("Unknown"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compatibility {
    Compatible,
    Unknown,
}

impl Compatibility {
    fn combine(self, other: Self) -> Self {
        if self == Self::Unknown || other == Self::Unknown {
            Self::Unknown
        } else {
            Self::Compatible
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeUnifyError {
    Mismatch {
        expected: TypeExpr,
        actual: TypeExpr,
    },
    VariableConflict {
        variable: String,
        previous: TypeExpr,
        required: TypeExpr,
    },
    RecursiveVariable {
        variable: String,
        ty: TypeExpr,
    },
}

impl fmt::Display for TypeUnifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch { expected, actual } => {
                write!(f, "expected {expected}, found {actual}")
            }
            Self::VariableConflict {
                variable,
                previous,
                required,
            } => write!(
                f,
                "type variable `{variable}` was inferred as {previous}, but now requires {required}"
            ),
            Self::RecursiveVariable { variable, ty } => {
                write!(
                    f,
                    "type variable `{variable}` cannot contain itself in {ty}"
                )
            }
        }
    }
}

impl std::error::Error for TypeUnifyError {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Substitutions(pub BTreeMap<String, TypeExpr>);

impl Substitutions {
    pub fn unify(
        &mut self,
        expected: &TypeExpr,
        actual: &TypeExpr,
    ) -> Result<Compatibility, TypeUnifyError> {
        // Unknown supplies no binding and does not prove incompatibility.
        if matches!(expected, TypeExpr::Unknown) || matches!(actual, TypeExpr::Unknown) {
            return Ok(Compatibility::Unknown);
        }
        match (expected, actual) {
            (TypeExpr::Variable(name), other) | (other, TypeExpr::Variable(name)) => {
                self.bind(name, other)
            }
            (TypeExpr::Named(left), TypeExpr::Named(right)) if left == right => {
                Ok(Compatibility::Compatible)
            }
            (
                TypeExpr::Applied {
                    constructor: left,
                    args: left_args,
                },
                TypeExpr::Applied {
                    constructor: right,
                    args: right_args,
                },
            ) if left == right && left_args.len() == right_args.len() => {
                let mut result = Compatibility::Compatible;
                for (left_arg, right_arg) in left_args.iter().zip(right_args) {
                    result = result.combine(self.unify(left_arg, right_arg)?);
                }
                Ok(result)
            }
            _ => Err(TypeUnifyError::Mismatch {
                expected: self.substitute(expected),
                actual: self.substitute(actual),
            }),
        }
    }

    pub fn substitute(&self, ty: &TypeExpr) -> TypeExpr {
        match ty {
            TypeExpr::Variable(name) => self
                .0
                .get(name)
                .map(|bound| self.substitute(bound))
                .unwrap_or_else(|| ty.clone()),
            TypeExpr::Applied { constructor, args } => TypeExpr::Applied {
                constructor: constructor.clone(),
                args: args.iter().map(|arg| self.substitute(arg)).collect(),
            },
            _ => ty.clone(),
        }
    }

    fn bind(&mut self, name: &str, ty: &TypeExpr) -> Result<Compatibility, TypeUnifyError> {
        let required = self.substitute(ty);
        if required == TypeExpr::Variable(name.to_owned()) {
            return Ok(Compatibility::Compatible);
        }
        if let Some(previous) = self.0.get(name).cloned() {
            let compatibility =
                self.unify(&previous, &required)
                    .map_err(|_| TypeUnifyError::VariableConflict {
                        variable: name.to_owned(),
                        previous: self.substitute(&previous),
                        required: required.clone(),
                    })?;
            // An earlier partial binding (for example, Frame<Unknown>) must
            // absorb details learned from later ports (Frame<Foo>).
            let refined = refine_known(&self.substitute(&previous), &self.substitute(&required));
            self.0.insert(name.to_owned(), refined);
            return Ok(compatibility);
        }
        if required.contains_variable(name) {
            return Err(TypeUnifyError::RecursiveVariable {
                variable: name.to_owned(),
                ty: required,
            });
        }
        self.0.insert(name.to_owned(), required);
        Ok(Compatibility::Compatible)
    }
}

fn refine_known(left: &TypeExpr, right: &TypeExpr) -> TypeExpr {
    match (left, right) {
        (TypeExpr::Unknown, other) | (other, TypeExpr::Unknown) => other.clone(),
        (
            TypeExpr::Applied {
                constructor,
                args: left_args,
            },
            TypeExpr::Applied {
                args: right_args, ..
            },
        ) => TypeExpr::applied(
            constructor,
            left_args
                .iter()
                .zip(right_args)
                .map(|(left, right)| refine_known(left, right))
                .collect(),
        ),
        (known, _) => known.clone(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeParseError {
    pub message: String,
    /// The byte range within the parsed text that the error is about.
    pub span: Range<usize>,
}

impl TypeParseError {
    fn new(span: Range<usize>, message: &str) -> Self {
        Self {
            message: message.to_owned(),
            span,
        }
    }
}

impl fmt::Display for TypeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TypeParseError {}

/// Product declarations treat bare names as constructors. In operation
/// signatures, a bare single uppercase letter denotes a local type variable.
/// A `$` prefix allows longer variable names without confusing them with
/// named types such as `Image` or `World`.
pub fn parse_type_expr(text: &str, signature: bool) -> Result<TypeExpr, TypeParseError> {
    struct Parser<'a> {
        text: &'a str,
        offset: usize,
        signature: bool,
    }

    impl Parser<'_> {
        fn skip_space(&mut self) {
            while self
                .text
                .as_bytes()
                .get(self.offset)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.offset += 1;
            }
        }

        fn take(&mut self, byte: u8) -> bool {
            self.skip_space();
            if self.text.as_bytes().get(self.offset) == Some(&byte) {
                self.offset += 1;
                true
            } else {
                false
            }
        }

        /// A one-byte span at the current offset, or an empty span at the
        /// end of the text when nothing is left to point at.
        fn here(&self) -> Range<usize> {
            let end = (self.offset + 1).min(self.text.len()).max(self.offset);
            self.offset..end
        }

        fn expression(&mut self) -> Result<TypeExpr, TypeParseError> {
            self.skip_space();
            let variable_marker = self.offset;
            let explicit_variable = self.take(b'$');
            let name_start = self.offset;
            let bytes = self.text.as_bytes();
            let valid_start = bytes
                .get(self.offset)
                .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_');
            while bytes
                .get(self.offset)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                self.offset += 1;
            }
            let token = variable_marker..self.offset;
            if explicit_variable && !self.signature {
                return Err(TypeParseError::new(
                    token,
                    "type variables are only allowed in operation signatures",
                ));
            }
            if !valid_start {
                return Err(TypeParseError::new(token, "expected type name"));
            }
            let name = &self.text[name_start..self.offset];
            if explicit_variable {
                if self.take(b'<') {
                    return Err(TypeParseError::new(
                        token,
                        "a type variable cannot have type arguments",
                    ));
                }
                Ok(TypeExpr::variable(name))
            } else if self.take(b'<') {
                let mut args = Vec::new();
                loop {
                    args.push(self.expression()?);
                    if self.take(b'>') {
                        break;
                    }
                    if !self.take(b',') {
                        return Err(TypeParseError::new(
                            self.here(),
                            "expected `,` or `>` in parameterized type",
                        ));
                    }
                }
                Ok(TypeExpr::applied(name, args))
            } else if name == "Unknown" {
                Ok(TypeExpr::Unknown)
            } else if self.signature && name.len() == 1 && name.as_bytes()[0].is_ascii_uppercase() {
                Ok(TypeExpr::variable(name))
            } else {
                Ok(TypeExpr::named(name))
            }
        }
    }

    let mut parser = Parser {
        text,
        offset: 0,
        signature,
    };
    let ty = parser.expression()?;
    parser.skip_space();
    if parser.offset != text.len() {
        return Err(TypeParseError::new(
            0..text.len(),
            "unexpected trailing text in type expression",
        ));
    }
    Ok(ty)
}
