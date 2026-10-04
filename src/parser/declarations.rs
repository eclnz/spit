//! Declarations shared by both forms: products, operations and their ports,
//! input bindings, commands, path rules and imports. A recipe's `require`
//! and conditional `exclude` rules are in `rules.rs`.

use std::collections::BTreeMap;

use crate::model::{Beside, DirectoryDiscovery, InputBinding, Invocation, ProductDef};
use crate::paths::{validate_discovery_rule, PathTemplate};
use crate::types::{parse_type_expr, TypeExpr, TypeParseError};

use super::check::{only_checks, split_clauses};
use super::lexical::{call_parts, comma_items, identifier, qualified_identifier, split_ending};
use super::source_map::tail_place;
use super::{ParseError, PathRule};

pub(super) fn single_colon(line: &str) -> Option<usize> {
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

/// Turn a [`TypeParseError`] into a [`ParseError`] pointing at the specific
/// token within `ty` that the type parser rejected, rather than all of `ty`.
pub(super) fn type_error(number: usize, ty: &str, error: TypeParseError) -> ParseError {
    ParseError::new(number, error.message).at_token(&ty[error.span])
}

/// The list after `dimensions`, as in `dimensions [model, config, seed]`.
pub(super) fn parse_dimension_order(text: &str, number: usize) -> Result<Vec<String>, ParseError> {
    let text = text.trim();
    let list = text
        .strip_prefix('[')
        .and_then(|list| list.strip_suffix(']'))
        .ok_or_else(|| {
            ParseError::new(number, "expected `dimensions [first, second, ...]`").at_token(text)
        })?;
    let mut order: Vec<String> = Vec::new();
    for dimension in comma_items(list, number)? {
        let dimension = identifier(dimension, number, "dimension")?;
        if order.iter().any(|known| known == dimension) {
            return Err(
                ParseError::new(number, format!("`dimensions` repeats `{dimension}`"))
                    .at_token(dimension),
            );
        }
        order.push(dimension.to_owned());
    }
    if order.is_empty() {
        return Err(
            ParseError::new(number, "`dimensions` needs at least one dimension").at_token(text),
        );
    }
    Ok(order)
}

pub(super) fn parse_product(line: &str, number: usize) -> Result<ProductDef, ParseError> {
    let (line, clauses) = split_clauses(line, "expected `@ check(...)` after a source", number)?;
    let checks = only_checks(&clauses, "a source", number)?;
    let (line, beside) = match line.rsplit_once(" beside ") {
        Some((item, sibling)) => {
            if item.trim_end().ends_with(']') {
                return Err(ParseError::new(number, "a source written `beside` another inherits its dimensions; remove its [dimensions]"));
            }
            let (item, suffix) = super::operation::beside_suffix(item.trim(), number, "source")?;
            let sibling = identifier(sibling.trim(), number, "source beside name")?;
            (item, Some((sibling, suffix)))
        }
        None => (line, None),
    };
    let (declaration, dimensions) = match line.split_once('[') {
        Some((declaration, dimensions)) => (declaration, Some(dimensions)),
        None => (line, None),
    };
    // `name : Type .ext`: the extension follows the type, as on an output,
    // and a `/` after it makes the source a folder.
    let (named, extension, folder) = split_ending(declaration, number)?;
    let (name, artifact_type) = if let Some((name, ty)) = named.split_once(':') {
        let ty = ty.trim();
        let ty = parse_type_expr(ty, false).map_err(|error| type_error(number, ty, error))?;
        (name.trim(), ty)
    } else {
        (named.trim(), TypeExpr::Unknown)
    };
    let name = identifier(name, number, "product name").map_err(|error| {
        if dimensions.is_none() && name.split_whitespace().count() > 1 {
            ParseError::new(
                number,
                "expected source name, optional `: Type`, optional `.ext`, optional `/` for a folder, and optional [dimensions]",
            )
            .at_token(name)
        } else {
            error
        }
    })?;
    let dimensions = if let Some(dimensions) = dimensions {
        // From the `[` that is never closed to the end of the declaration.
        let bracketed = &line[declaration.len()..];
        let dimensions = dimensions.strip_suffix(']').ok_or_else(|| {
            ParseError::new(number, "expected closing `]` in product declaration")
                .at_token(bracketed)
        })?;
        let items = comma_items(dimensions, number)?;
        if items.is_empty() {
            return Err(ParseError::new(
                number,
                format!("`{name}` has no dimensions, so it takes no brackets; remove `[]`"),
            )
            .at_token(bracketed));
        }
        items
    } else {
        Vec::new()
    };
    for dimension in &dimensions {
        identifier(dimension, number, "dimension")?;
    }
    let mut product = ProductDef::new(name, artifact_type, &dimensions);
    product.extension = extension.map(str::to_owned);
    product.folder = folder;
    product.checks = checks;
    if let Some((sibling, suffix)) = beside {
        if product.folder || !product.dimensions.is_empty() || product.extension.is_some() {
            return Err(ParseError::new(
                number,
                "a source written `beside` another inherits its dimensions and names only a suffix, as in `source meta : Json .json beside image`",
            ));
        }
        product.extension = suffix.find('.').map(|dot| suffix[dot..].to_owned());
        product.beside = Some(Beside {
            sibling: sibling.to_owned(),
            suffix,
        });
    }
    Ok(product)
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
                if !binding.vary.is_empty() {
                    return Err(ParseError::new(
                        number,
                        "write the dimensions in one clause: `@ vary(model, config)`",
                    )
                    .at_token(selector));
                }
                for dimension in items {
                    let dimension = identifier(dimension, number, "vary dimension")?;
                    if binding.vary.iter().any(|name| name == dimension) {
                        return Err(ParseError::new(
                            number,
                            format!("`@ vary(...)` repeats `{dimension}`"),
                        )
                        .at_token(selector));
                    }
                    binding.vary.push(dimension.to_owned());
                }
            }
            "where" => {
                if !binding.pinned.is_empty() {
                    return Err(duplicate());
                }
                binding.pinned = parse_pins(&items, number)?;
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
                binding.each = parse_each(&items, number)?;
            }
            _ => return Err(ParseError::new(number, SELECTORS).at_token(keyword)),
        }
    }
    Ok(binding)
}

/// The `dimension=value` pins of `@ where(...)`, each dimension once.
fn parse_pins(items: &[&str], number: usize) -> Result<BTreeMap<String, String>, ParseError> {
    let mut pinned = BTreeMap::new();
    for &item in items {
        let (dimension, value) = item.split_once('=').ok_or_else(|| {
            ParseError::new(number, "expected `dimension=value` in `@ where(...)`").at_token(item)
        })?;
        let dimension = identifier(dimension.trim(), number, "where dimension")?;
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(
                ParseError::new(number, "a `@ where` value must be one nonempty token")
                    .at_token(item),
            );
        }
        if pinned
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
    Ok(pinned)
}

/// The dimensions of `@ each(...)`, each once.
fn parse_each(items: &[&str], number: usize) -> Result<Vec<String>, ParseError> {
    let mut each: Vec<String> = Vec::new();
    for &item in items {
        let dimension = identifier(item, number, "each dimension")?;
        if each.iter().any(|named| named == dimension) {
            return Err(ParseError::new(
                number,
                format!("`@ each(...)` names `{dimension}` twice"),
            )
            .at_token(item));
        }
        each.push(dimension.to_owned());
    }
    Ok(each)
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// What an `exclude` line names: a file of rules, or one rule.
#[derive(Clone, Debug)]
pub(crate) enum ExcludeLine {
    /// `exclude from qc/excluded.csv`, relative to the recipe's folder.
    File(String),
    /// A source, the values an artifact's identity must include, or both,
    /// the values in the order written.
    Rule {
        product: Option<String>,
        values: Vec<(String, String)>,
    },
}

/// Parse the text after `exclude`: `from <file>`, or a source with or
/// without `[dimension=value,...]`, or `[dimension=value,...]` alone.
pub(crate) fn parse_exclude(text: &str, number: usize) -> Result<ExcludeLine, ParseError> {
    let text = text.trim();
    if text == "from" {
        return Err(ParseError::new(
            number,
            "expected one file after `exclude from`, such as `exclude from qc/excluded.csv`",
        ));
    }
    if let Some(file) = text.strip_prefix("from ") {
        let file = file.trim();
        if file.is_empty() || file.chars().any(char::is_whitespace) {
            return Err(ParseError::new(
                number,
                "expected one file after `exclude from`, such as `exclude from qc/excluded.csv`",
            ));
        }
        return Ok(ExcludeLine::File(file.to_owned()));
    }
    let (product, bindings) = match text.split_once('[') {
        Some((product, bindings)) => (product.trim(), Some(bindings)),
        None => (text, None),
    };
    let product = if product.is_empty() {
        None
    } else {
        Some(qualified_identifier(product, number, "source product")?.to_owned())
    };
    let values = match bindings {
        Some(bindings) => {
            // Checked as a record's values are, then kept in written order.
            super::inventory::parse_bindings(bindings, number)?;
            let inner = bindings.trim_end().trim_end_matches(']');
            comma_items(inner, number)?
                .into_iter()
                .filter_map(|item| item.split_once('='))
                .map(|(dimension, value)| (dimension.trim().to_owned(), value.trim().to_owned()))
                .collect()
        }
        None => Vec::new(),
    };
    if product.is_none() && values.is_empty() {
        return Err(ParseError::new(
            number,
            "`exclude` names a source, values such as `[sub=02]`, or both",
        ));
    }
    Ok(ExcludeLine::Rule { product, values })
}
