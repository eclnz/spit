//! Declarations shared by both forms: products, operations and their ports,
//! input bindings, coverage rules, commands, path rules and imports.

use std::collections::BTreeMap;

use crate::command::CommandTemplate;
use crate::model::{
    CommandDef, CommandRole, CountRequirement, CoverageAction, CoverageRule, DirectoryDiscovery,
    InputBinding, Invocation, ProductDef,
};
use crate::paths::{validate_discovery_rule, PathTemplate};
use crate::types::{parse_type_expr, TypeExpr, TypeParseError};

use super::lexical::{call_parts, comma_items, extension, identifier, qualified_identifier};
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
    if let Some(rest) = line.strip_prefix("drop ") {
        return parse_drop(rest, number);
    }
    if let Some(rest) = line.strip_prefix("skip ") {
        return Err(skip_replaced(rest, number));
    }
    let syntax =
        "expected constraint: require product count=1 per [dimensions], or dimension=value,...";
    let rest = line
        .strip_prefix("require ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let (target, terms, dimensions) = parse_require_parts(rest, number, syntax)?;
    let mut rule = CoverageRule::new(target, &dimensions, CountRequirement::AtLeast(1));
    rule.count = terms.count;
    rule.values = owned_values(terms.values);
    Ok(rule)
}

/// A `require` rule's source, its terms, and the dimensions after `per`.
fn parse_require_parts<'a>(
    rest: &'a str,
    number: usize,
    syntax: &str,
) -> Result<(&'a str, RuleTerms<'a>, Vec<&'a str>), ParseError> {
    let (subject, dimensions) = rest
        .split_once(" per ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let mut parts = subject.split_whitespace();
    let product = qualified_identifier(parts.next().unwrap_or(""), number, "constraint product")?;
    let terms = parse_rule_terms(parts, number, syntax)?;
    let dimensions = parse_group_dimensions(dimensions, number)?;
    Ok((product, terms, dimensions))
}

/// Parse the text after `drop`: `[dimensions] where source condition`, the
/// condition one of `count<2` (any comparison), `missing dimension=value,...`
/// or `has dimension=value,...`.
fn parse_drop(rest: &str, number: usize) -> Result<CoverageRule, ParseError> {
    let syntax = "expected `drop [dimensions] where source` and one condition: `count<2`, `missing run=1,2` or `has run=3`";
    let rest = rest.trim_start();
    let close = rest
        .find(']')
        .filter(|_| rest.starts_with('['))
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let dimensions = parse_group_dimensions(&rest[..=close], number)?;
    let after = rest[close + 1..].trim_start();
    let after = after.strip_prefix("where ").ok_or_else(|| {
        ParseError::new(
            number,
            "expected `where` after the groups, as in `drop [sub] where sessions count<2`",
        )
    })?;
    let mut tokens = after.split_whitespace();
    let product = qualified_identifier(tokens.next().unwrap_or(""), number, "constraint product")?;
    let mut rule = CoverageRule::new(product, &dimensions, CountRequirement::AtLeast(1));
    rule.action = CoverageAction::Drop;
    rule.count = None;
    let condition = tokens
        .next()
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let listed: Vec<&str> = tokens.collect();
    match condition {
        "missing" | "has" => {
            if listed.is_empty() {
                return Err(ParseError::new(
                    number,
                    format!("expected values after `{condition}`, such as `{condition} run=1,2`"),
                ));
            }
            let values = owned_values(parse_rule_terms(listed.into_iter(), number, syntax)?.values);
            if condition == "missing" {
                rule.values = values;
            } else {
                rule.has = values;
            }
        }
        token => {
            let count = parse_count_term(token, number)
                .transpose()?
                .ok_or_else(|| ParseError::new(number, syntax).at_token(token))?;
            if let Some(extra) = listed.first() {
                return Err(
                    ParseError::new(number, "a `drop` rule takes one condition").at_token(extra)
                );
            }
            rule.count = Some(count);
        }
    }
    Ok(rule)
}

/// The error for a `skip` rule, which `drop` replaces, with the `drop` rule
/// that removes the same groups when the old rule reads cleanly.
fn skip_replaced(rest: &str, number: usize) -> ParseError {
    let old = "expected `skip product count>=2 per [dimensions]`";
    let suggestion =
        parse_require_parts(rest, number, old)
            .ok()
            .map(|(product, terms, dimensions)| {
                let groups = dimensions.join(", ");
                let mut lines = Vec::new();
                if let Some(count) = terms.count {
                    lines.push(format!(
                        "drop [{groups}] where {product} {}",
                        count.negated().as_written()
                    ));
                }
                for (dimension, values) in &terms.values {
                    lines.push(format!(
                        "drop [{groups}] where {product} missing {dimension}={}",
                        values.join(",")
                    ));
                }
                lines.join("` and `")
            });
    let message = match suggestion {
        Some(rule) => format!(
            "`skip` is replaced by `drop`, which names the groups to remove: write `{rule}`"
        ),
        None => "`skip` is replaced by `drop`, which names the groups to remove, as in `drop [sub] where sessions count<2`".to_owned(),
    };
    ParseError::new(number, message)
}

/// A term `count` followed by a comparison and a number, such as `count<2`;
/// `None` when `token` is not one.
fn parse_count_term(token: &str, number: usize) -> Option<Result<CountRequirement, ParseError>> {
    let rest = token.strip_prefix("count")?;
    /// Makes the comparison a written operator names, given its count.
    type Comparison = fn(usize) -> CountRequirement;
    // Longer operators first, so `>=` is not read as `>`.
    const COMPARISONS: [(&str, Comparison); 6] = [
        ("!=", CountRequirement::NotExactly),
        (">=", CountRequirement::AtLeast),
        ("<=", CountRequirement::AtMost),
        ("=", CountRequirement::Exactly),
        (">", CountRequirement::MoreThan),
        ("<", CountRequirement::FewerThan),
    ];
    let (op, value) = COMPARISONS
        .iter()
        .find_map(|(op, make)| Some((make, rest.strip_prefix(op)?)))?;
    Some(parse_count(value, number).map(op))
}

fn owned_values(values: BTreeMap<String, Vec<&str>>) -> BTreeMap<String, Vec<String>> {
    values
        .into_iter()
        .map(|(dimension, listed)| (dimension, listed.into_iter().map(str::to_owned).collect()))
        .collect()
}

/// A rule's count term and its `dimension=value,...` terms.
struct RuleTerms<'a> {
    count: Option<CountRequirement>,
    values: BTreeMap<String, Vec<&'a str>>,
}

/// A rule's terms after its product; it needs at least one.
fn parse_rule_terms<'a>(
    terms: impl Iterator<Item = &'a str>,
    number: usize,
    syntax: &str,
) -> Result<RuleTerms<'a>, ParseError> {
    let mut count = None;
    let mut values = BTreeMap::new();
    for token in terms {
        if let Some(parsed) = parse_count_term(token, number) {
            if count.replace(parsed?).is_some() {
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
    Ok(RuleTerms { count, values })
}

/// The `[dimension, ...]` a rule groups by.
fn parse_group_dimensions(dimensions: &str, number: usize) -> Result<Vec<&str>, ParseError> {
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
    Ok(dimensions)
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
    let (declaration, dimensions) = match line.split_once('[') {
        Some((declaration, dimensions)) => (declaration, Some(dimensions)),
        None => (line, None),
    };
    // `name : Type .ext`: the extension follows the type, as on an output.
    let (named, extension) = match declaration.split_once('.') {
        Some((named, _)) => {
            let written = &declaration[named.len()..];
            (named, Some(extension(written.trim(), number)?.to_owned()))
        }
        None => (declaration, None),
    };
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
                "expected source name, optional `: Type`, optional `.ext`, and optional [dimensions]",
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
    product.extension = extension;
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
