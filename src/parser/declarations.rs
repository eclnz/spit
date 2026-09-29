//! Declarations shared by both forms: products, operations and their ports,
//! input bindings, coverage rules, commands, path rules and imports.

use std::collections::BTreeMap;

use crate::command::CommandTemplate;
use crate::model::{
    CommandDef, CommandRole, CountRequirement, CoverageRule, DirectoryDiscovery, InputBinding,
    Invocation, ProductDef,
};
use crate::paths::{validate_discovery_rule, PathTemplate};
use crate::types::{parse_type_expr, TypeExpr, TypeParseError};

use super::lexical::{call_parts, comma_items, identifier, qualified_identifier};
use super::source_map::tail_place;
use super::{ParseError, PathRule};

pub(super) fn parse_command(
    line: &str,
    number: usize,
    role: CommandRole,
) -> Result<CommandDef, ParseError> {
    let delimiter = declaration_delimiter(line).ok_or_else(|| {
        ParseError::new(
            number,
            match role {
                CommandRole::Run => "expected command: operation: executable [arguments]",
                CommandRole::Verify => "expected verify operation: executable [arguments]",
            },
        )
    })?;
    let (operation, rest) = line.split_at(delimiter);
    let operation = qualified_identifier(operation.trim(), number, "command operation")?;
    let template = rest[1..].trim();
    if template.is_empty() {
        return Err(ParseError::new(
            number,
            "command template must not be empty",
        ));
    }
    let parsed = CommandTemplate::parse(template).map_err(|error| {
        ParseError::new(
            number,
            format!("command `{operation}`: {}", error.message()),
        )
        .at_token(template)
    })?;
    Ok(CommandDef {
        role,
        ..CommandDef::new(operation, parsed)
    })
}

fn declaration_delimiter(line: &str) -> Option<usize> {
    [single_colon(line), line.find('=')]
        .into_iter()
        .flatten()
        .min()
}

fn single_colon(line: &str) -> Option<usize> {
    line.char_indices().find_map(|(index, character)| {
        (character == ':' && !line[..index].ends_with(':') && !line[index + 1..].starts_with(':'))
            .then_some(index)
    })
}

/// Parse a path rule. A default `path:` inside `stage` covers only that
/// stage's products.
pub(super) fn parse_path(
    stage: Option<String>,
    original: &str,
    line: &str,
    number: usize,
) -> Result<PathRule, ParseError> {
    let (product, template) = if let Some(template) = line.strip_prefix("path:") {
        (None, template)
    } else {
        let declaration = line.strip_prefix("path ").unwrap_or("");
        let delimiter = single_colon(declaration)
            .ok_or_else(|| ParseError::new(number, "expected `:` after path product name"))?;
        let (product, template) = declaration.split_at(delimiter);
        (
            Some(qualified_identifier(product.trim(), number, "path product")?.to_owned()),
            &template[1..],
        )
    };
    let template = template.trim();
    let template = template
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            template
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(template)
        .trim();
    if template.is_empty() {
        return Err(ParseError::new(number, "path template must not be empty"));
    }
    let parsed = PathTemplate::parse(template)
        .map_err(|error| ParseError::new(number, error.message()).at_token(template))?;
    Ok(PathRule {
        stage: stage.filter(|_| product.is_none()),
        product,
        template: parsed,
        place: tail_place(original, number, template),
    })
}

pub(super) fn parse_discover(line: &str, number: usize) -> Result<DirectoryDiscovery, ParseError> {
    let (declaration, pattern) = line.split_once(" from dirs ").ok_or_else(|| {
        ParseError::new(
            number,
            "expected `discover name: [dimensions] from dirs path-pattern`",
        )
    })?;
    let (name, dimensions) = declaration
        .split_once(':')
        .ok_or_else(|| ParseError::new(number, "expected `:` after discovery name"))?;
    let name = identifier(name.trim(), number, "discovery name")?;
    let dimensions = dimensions.trim();
    let dimensions = dimensions
        .strip_prefix('[')
        .and_then(|items| items.strip_suffix(']'))
        .ok_or_else(|| ParseError::new(number, "expected `[dimensions]` after discovery name"))?;
    let dimensions = comma_items(dimensions, number)?;
    for dimension in &dimensions {
        identifier(dimension, number, "dimension")?;
    }
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err(ParseError::new(
            number,
            "discovery directory pattern must not be empty",
        ));
    }
    let template = PathTemplate::parse(pattern)
        .map_err(|error| ParseError::new(number, error.message()).at_token(pattern))?;
    let discovery = DirectoryDiscovery {
        name: name.to_owned(),
        dimensions: dimensions.into_iter().map(str::to_owned).collect(),
        template,
    };
    validate_discovery_rule(&discovery)
        .map_err(|error| ParseError::new(number, error.message()).at_token(pattern))?;
    Ok(discovery)
}

#[derive(Debug)]
pub(crate) struct UseSpec {
    pub(crate) names: Option<Vec<String>>,
    pub(crate) path: String,
    pub(crate) alias: Option<String>,
}

pub(crate) fn parse_use(line: &str, number: usize) -> Result<UseSpec, ParseError> {
    let syntax = "expected `use path [as alias]` or `use name[, name] from path [as alias]`";
    let rest = line
        .strip_prefix("use ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let (names, path_and_alias) = if rest.starts_with('"') || rest.starts_with('\'') {
        (None, rest)
    } else if let Some((names, path)) = rest.split_once(" from ") {
        let names = comma_items(names, number)?;
        if names.is_empty() {
            return Err(ParseError::new(number, syntax));
        }
        for name in &names {
            qualified_identifier(name, number, "import name")?;
        }
        (Some(names.into_iter().map(str::to_owned).collect()), path)
    } else {
        (None, rest)
    };
    let (path, alias) = parse_use_path(path_and_alias, number)?;
    if path.is_empty() {
        return Err(ParseError::new(number, "import needs a file path"));
    }
    Ok(UseSpec { names, path, alias })
}

fn parse_use_path(text: &str, number: usize) -> Result<(String, Option<String>), ParseError> {
    let text = text.trim();
    if let Some(quote @ ('"' | '\'')) = text.chars().next() {
        let closing = text[1..]
            .find(quote)
            .map(|index| index + 1)
            .ok_or_else(|| ParseError::new(number, "unterminated quoted import path"))?;
        let path = &text[1..closing];
        let tail = text[closing + 1..].trim();
        let alias = if tail.is_empty() {
            None
        } else {
            let value = tail
                .strip_prefix("as ")
                .ok_or_else(|| ParseError::new(number, "expected `as alias` after import path"))?;
            Some(identifier(value.trim(), number, "import alias")?.to_owned())
        };
        Ok((path.to_owned(), alias))
    } else if let Some((path, alias)) = text.rsplit_once(" as ") {
        Ok((
            path.trim().to_owned(),
            Some(identifier(alias.trim(), number, "import alias")?.to_owned()),
        ))
    } else {
        Ok((text.to_owned(), None))
    }
}

pub(super) fn parse_coverage_rule(line: &str, number: usize) -> Result<CoverageRule, ParseError> {
    let syntax = "expected constraint: require product count=1 per [dimensions], count>=1, or dimension=value,...";
    let rest = line
        .strip_prefix("require ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let (subject, dimensions) = rest
        .split_once(" per ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let mut parts = subject.split_whitespace();
    let product = qualified_identifier(parts.next().unwrap_or(""), number, "constraint product")?;
    let mut count = None;
    let mut values = BTreeMap::new();
    for token in parts {
        let parsed = if let Some(value) = token.strip_prefix("count=") {
            Some(CountRequirement::Exactly(parse_count(value, number)?))
        } else if let Some(value) = token.strip_prefix("count>=") {
            Some(CountRequirement::AtLeast(parse_count(value, number)?))
        } else {
            None
        };
        if let Some(parsed) = parsed {
            if count.replace(parsed).is_some() {
                return Err(ParseError::new(number, "a rule takes one count").at_token(token));
            }
        } else if let Some((dimension, listed)) = token.split_once('=') {
            let dimension = identifier(dimension, number, "required dimension")?;
            let listed: Vec<_> = listed.split(',').collect();
            if listed.iter().any(|value| value.is_empty()) {
                return Err(
                    ParseError::new(number, "required values must not be empty").at_token(token)
                );
            }
            if values.insert(dimension.to_owned(), listed).is_some() {
                return Err(ParseError::new(
                    number,
                    format!("duplicate required dimension `{dimension}`"),
                )
                .at_token(token));
            }
        } else {
            return Err(ParseError::new(number, syntax).at_token(token));
        }
    }
    if count.is_none() && values.is_empty() {
        return Err(ParseError::new(number, syntax));
    }
    let bracketed = dimensions.trim();
    let dimensions = bracketed.strip_prefix('[').ok_or_else(|| {
        ParseError::new(number, "expected `[` before constraint dimensions").at_token(bracketed)
    })?;
    let dimensions = dimensions.strip_suffix(']').ok_or_else(|| {
        ParseError::new(number, "expected closing `]` in constraint dimensions").at_token(bracketed)
    })?;
    let dimensions = comma_items(dimensions, number)?;
    if dimensions.is_empty() {
        return Err(ParseError::new(
            number,
            "coverage rule needs group dimensions",
        ));
    }
    for dimension in &dimensions {
        identifier(dimension, number, "constraint dimension")?;
    }
    let mut rule = CoverageRule::new(
        product,
        &dimensions,
        count.unwrap_or(CountRequirement::AtLeast(1)),
    );
    for (dimension, listed) in values {
        rule = rule.requiring(dimension, listed);
    }
    Ok(rule)
}

fn parse_count(value: &str, number: usize) -> Result<usize, ParseError> {
    value.parse().map_err(|_| {
        ParseError::new(number, "constraint count must be a nonnegative integer").at_token(value)
    })
}

/// Turn a [`TypeParseError`] into a [`ParseError`] pointing at the specific
/// token within `ty` that the type parser rejected, rather than all of `ty`.
pub(super) fn type_error(number: usize, ty: &str, error: TypeParseError) -> ParseError {
    ParseError::new(number, error.message).at_token(&ty[error.span])
}

pub(super) fn parse_product(line: &str, number: usize) -> Result<ProductDef, ParseError> {
    let (declaration, dimensions) = line
        .split_once('[')
        .ok_or_else(|| ParseError::new(number, "expected product name followed by [dimensions]"))?;
    let (name, artifact_type) = if let Some((name, ty)) = declaration.split_once(':') {
        let ty = ty.trim();
        let ty = parse_type_expr(ty, false).map_err(|error| type_error(number, ty, error))?;
        (name.trim(), ty)
    } else {
        (declaration.trim(), TypeExpr::Unknown)
    };
    let name = identifier(name, number, "product name")?;
    // From the `[` that is never closed to the end of the declaration.
    let bracketed = &line[declaration.len()..];
    let dimensions = dimensions.strip_suffix(']').ok_or_else(|| {
        ParseError::new(number, "expected closing `]` in product declaration").at_token(bracketed)
    })?;
    let dimensions = comma_items(dimensions, number)?;
    for dimension in &dimensions {
        identifier(dimension, number, "dimension")?;
    }
    Ok(ProductDef::new(name, artifact_type, &dimensions))
}

pub(super) fn parse_invocation_parts(
    outputs: Vec<String>,
    call: &str,
    number: usize,
) -> Result<Invocation, ParseError> {
    let (operation, args) = call_parts(call.trim(), number)?;
    let bindings = comma_items(args, number)?
        .into_iter()
        .map(|arg| parse_binding(arg, number))
        .collect::<Result<_, _>>()?;
    Ok(Invocation::with_outputs(operation, bindings, outputs))
}

const SELECTORS: &str = "expected `@ vary(dimension)`, `@ where(dimension=value, ...)`, `@ same(dimension, ...)`, or `@ each(dimension, ...)`";

/// Parse `product [@ selector(...)]...`.
fn parse_binding(arg: &str, number: usize) -> Result<InputBinding, ParseError> {
    let mut parts = arg.split('@');
    let product = parts.next().unwrap_or_default().trim();
    let mut binding =
        InputBinding::product(qualified_identifier(product, number, "input product")?);
    for selector in parts {
        let selector = selector.trim();
        let (keyword, arguments) = call_parts(selector, number)
            .map_err(|_| ParseError::new(number, SELECTORS).at_token(selector))?;
        let items = comma_items(arguments, number)?;
        if items.is_empty() {
            return Err(
                ParseError::new(number, format!("`@ {keyword}()` needs a dimension"))
                    .at_token(selector),
            );
        }
        let duplicate =
            || ParseError::new(number, format!("duplicate `@ {keyword}(...)`")).at_token(keyword);
        match keyword {
            "vary" => {
                let [dimension] = items.as_slice() else {
                    return Err(ParseError::new(number, "`@ vary(...)` takes one dimension")
                        .at_token(selector));
                };
                let dimension = identifier(dimension, number, "vary dimension")?;
                if binding.vary.replace(dimension.to_owned()).is_some() {
                    return Err(duplicate());
                }
            }
            "where" => {
                if !binding.pinned.is_empty() {
                    return Err(duplicate());
                }
                for item in items {
                    let (dimension, value) = item.split_once('=').ok_or_else(|| {
                        ParseError::new(number, "expected `dimension=value` in `@ where(...)`")
                            .at_token(item)
                    })?;
                    let dimension = identifier(dimension.trim(), number, "where dimension")?;
                    let value = value.trim();
                    if value.is_empty() || value.chars().any(char::is_whitespace) {
                        return Err(ParseError::new(
                            number,
                            "a `@ where` value must be one nonempty token",
                        )
                        .at_token(item));
                    }
                    if binding
                        .pinned
                        .insert(dimension.to_owned(), value.to_owned())
                        .is_some()
                    {
                        return Err(ParseError::new(
                            number,
                            format!("`@ where(...)` pins `{dimension}` twice"),
                        )
                        .at_token(item));
                    }
                }
            }
            "same" => {
                let dimensions = items
                    .into_iter()
                    .map(|item| identifier(item, number, "same dimension"))
                    .collect::<Result<Vec<_>, _>>()?;
                if binding.same.replace(owned(&dimensions)).is_some() {
                    return Err(duplicate());
                }
            }
            "each" => {
                if !binding.each.is_empty() {
                    return Err(duplicate());
                }
                for item in items {
                    let dimension = identifier(item, number, "each dimension")?;
                    if binding.each.iter().any(|each| each == dimension) {
                        return Err(ParseError::new(
                            number,
                            format!("`@ each(...)` names `{dimension}` twice"),
                        )
                        .at_token(item));
                    }
                    binding.each.push(dimension.to_owned());
                }
            }
            _ => return Err(ParseError::new(number, SELECTORS).at_token(keyword)),
        }
    }
    Ok(binding)
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
