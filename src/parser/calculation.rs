//! Parse a calculation's infix syntax before it reaches a backend.

use crate::model::{CommandDef, CommandRole};
use crate::template::{CommandTemplate, Expression};

use super::declarations::single_colon;
use super::lexical::qualified_identifier;
use super::source_map::{tail_place, CommandPlaces};
use super::ParseError;

pub(super) fn parse(
    original: &str,
    declaration: &str,
    number: usize,
) -> Result<(CommandDef, CommandPlaces), ParseError> {
    let fail = |message: &str, token: &str| {
        ParseError::of(
            number,
            super::ParseErrorKind::Other {
                message: message.to_owned(),
            },
        )
        .at_token(token)
    };
    let delimiter = single_colon(declaration)
        .ok_or_else(|| fail("write `calculate operation: expression`", declaration))?;
    let operation = qualified_identifier(
        declaration[..delimiter].trim(),
        number,
        "calculation operation",
    )?;
    let text = declaration[delimiter + 1..].trim();
    let mut parser = Formula {
        rest: text,
        remaining: 64,
    };
    let expression = parser
        .expression(0)
        .map_err(|message| fail(&message, text))?;
    if !parser.rest.trim().is_empty() {
        return Err(fail(
            "expected the end of the calculation",
            parser.rest.trim(),
        ));
    }
    let place = tail_place(original, number, text);
    Ok((
        CommandDef {
            operation: operation.to_owned(),
            role: CommandRole::Run,
            lines: vec![CommandTemplate::calculation(text, expression)],
        },
        CommandPlaces {
            header: place.clone(),
            lines: vec![place],
        },
    ))
}

struct Formula<'a> {
    rest: &'a str,
    // Bounds both the parser's recursion and the depth of the resulting tree.
    remaining: usize,
}

impl<'a> Formula<'a> {
    fn take(&mut self, prefix: char) -> bool {
        self.rest = self.rest.trim_start();
        if let Some(rest) = self.rest.strip_prefix(prefix) {
            self.rest = rest;
            return true;
        }
        false
    }

    fn count(&mut self) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| "calculation is too long".to_owned())?;
        Ok(())
    }

    fn expression(&mut self, minimum: u8) -> Result<Expression, String> {
        self.count()?;
        let mut left = self.operand()?;
        loop {
            self.rest = self.rest.trim_start();
            let (operator, left_power, right_power) = match self.rest.as_bytes().first() {
                Some(b'+') => ("+", 1, 2),
                Some(b'-') => ("-", 1, 2),
                Some(b'*') => ("*", 3, 4),
                Some(b'/') => ("/", 3, 4),
                Some(b'^') => ("^", 6, 6),
                _ => break,
            };
            if left_power < minimum {
                break;
            }
            self.rest = &self.rest[1..];
            let right = self.expression(right_power)?;
            left = Expression::Apply(operator, vec![left, right]);
        }
        Ok(left)
    }

    fn operand(&mut self) -> Result<Expression, String> {
        if self.take('(') {
            let inner = self.expression(0)?;
            self.close()?;
            return Ok(inner);
        }
        if self.take('-') {
            return Ok(Expression::Apply("neg", vec![self.expression(5)?]));
        }
        if self.take('+') {
            return self.expression(5);
        }
        if self
            .rest
            .starts_with(|c: char| c.is_ascii_digit() || c == '.')
        {
            return self.number();
        }
        let name = self.consume(|c| c.is_ascii_alphanumeric() || c == '_');
        if name.is_empty() {
            return Err("expected a number, input port or function in the calculation".to_owned());
        }
        if !self.take('(') {
            return Ok(Expression::Input(name.to_owned()));
        }
        // Keep in step with the runner's OPERATORS in calculation.py.
        let (function, arity) = match name {
            "exp" => ("exp", 1),
            "log" => ("log", 1),
            "sqrt" => ("sqrt", 1),
            "abs" => ("abs", 1),
            "min" => ("min", 2),
            "max" => ("max", 2),
            "round" => ("round", 2),
            _ => return Err(format!("unknown calculation function `{name}`")),
        };
        let mut operands = Vec::with_capacity(arity);
        if !self.take(')') {
            loop {
                operands.push(self.expression(0)?);
                if !self.take(',') {
                    break;
                }
            }
            self.close()?;
        }
        if operands.len() != arity {
            return Err(format!(
                "`{function}` takes {arity} arguments, got {}",
                operands.len()
            ));
        }
        Ok(Expression::Apply(function, operands))
    }

    fn close(&mut self) -> Result<(), String> {
        self.take(')')
            .then_some(())
            .ok_or_else(|| "expected `)` in the calculation".to_owned())
    }

    fn consume(&mut self, valid: impl Fn(char) -> bool) -> &'a str {
        let end = self.rest.find(|c| !valid(c)).unwrap_or(self.rest.len());
        let (text, rest) = self.rest.split_at(end);
        self.rest = rest;
        text
    }

    fn number(&mut self) -> Result<Expression, String> {
        let start = self.rest;
        let digits = |c: char| c.is_ascii_digit();
        self.consume(digits);
        if let Some(rest) = self.rest.strip_prefix('.') {
            self.rest = rest;
            self.consume(digits);
        }
        if let Some(rest) = self.rest.strip_prefix(['e', 'E']) {
            self.rest = rest.strip_prefix(['+', '-']).unwrap_or(rest);
            self.consume(digits);
        }
        let written = &start[..start.len() - self.rest.len()];
        let number = written
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("`{written}` is not a finite decimal"))?;
        let decimal = format!("{number:?}");
        Ok(Expression::Number(
            decimal.strip_suffix(".0").unwrap_or(&decimal).to_owned(),
        ))
    }
}
