//! A recipe's coverage rules: `require` and `drop`, which both name their
//! groups first, then `where` and the source they count, and the errors for
//! a rule written in another rule's order or in an old form.

use std::collections::BTreeMap;

use crate::model::{CountRequirement, CoverageAction, CoverageRule};

use super::lexical::{comma_items, identifier, qualified_identifier};
use super::ParseError;

pub(super) fn parse_coverage_rule(line: &str, number: usize) -> Result<CoverageRule, ParseError> {
    if let Some(rest) = line.strip_prefix("drop ") {
        return parse_drop(rest, number);
    }
    if let Some(rest) = line.strip_prefix("skip ") {
        return Err(skip_replaced(rest, number));
    }
    let syntax = "expected `require [dimensions] where source` and a condition: `count=1`, `has run=1,2`, or both";
    let rest = line
        .strip_prefix("require ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    parse_require(rest, number, syntax)
}

/// Parse the text after `require`: `[dimensions] where source`, then a
/// count, `has dimension=value,...`, or both, in that order.
fn parse_require(rest: &str, number: usize, syntax: &str) -> Result<CoverageRule, ParseError> {
    let rest = rest.trim_start();
    if !rest.starts_with('[') {
        return Err(
            require_in_old_order(rest, number).unwrap_or_else(|| ParseError::new(number, syntax))
        );
    }
    let example = "require [sub, ses] where t1w count=1";
    let (dimensions, product, tokens) = parse_groups_where(rest, number, example, syntax)?;
    let mut rule = CoverageRule::new(product, &dimensions, CountRequirement::AtLeast(1));
    rule.count = None;
    let tokens: Vec<&str> = tokens.collect();
    let mut rest = &tokens[..];
    if let Some((token, after)) = rest
        .split_first()
        .filter(|(token, _)| token.starts_with("count"))
    {
        let count = parse_count_term(token, number)
            .transpose()?
            .ok_or_else(|| ParseError::new(number, syntax).at_token(token))?;
        rule.count = Some(count);
        rest = after;
    }
    match rest.split_first() {
        None if rule.count.is_none() => return Err(ParseError::new(number, syntax)),
        None => {}
        Some((&"has", values)) => {
            if values.is_empty() {
                return Err(ParseError::new(
                    number,
                    "expected values after `has`, such as `has run=1,2`",
                ));
            }
            let terms = parse_rule_terms(values.iter().copied(), number, syntax)?;
            if terms.count.is_some() {
                return Err(ParseError::new(number, "write the count before `has`, as in `require [sub] where bold count>=2 has run=1,2`"));
            }
            rule.values = owned_values(terms.values);
        }
        Some((token, _)) if token.contains('=') => {
            return Err(ParseError::new(
                number,
                format!("required values follow `has`: write `has {token}`"),
            )
            .at_token(token));
        }
        Some((token, _)) => return Err(ParseError::new(number, syntax).at_token(token)),
    }
    Ok(rule)
}

/// The groups, `where`, and source that begin a `drop` or `require` rule,
/// with the tokens after the source. `example` shows the shape when `where`
/// is missing.
fn parse_groups_where<'a>(
    rest: &'a str,
    number: usize,
    example: &str,
    syntax: &str,
) -> Result<(Vec<&'a str>, &'a str, std::str::SplitWhitespace<'a>), ParseError> {
    if !rest.starts_with('[') {
        return Err(ParseError::new(number, syntax));
    }
    // Without a `]`, the dimensions' own check says so, from the `[`.
    let close = match rest.find(']') {
        Some(close) => close,
        None => {
            return Err(parse_group_dimensions(rest, number)
                .err()
                .unwrap_or_else(|| ParseError::new(number, syntax)))
        }
    };
    let dimensions = parse_group_dimensions(&rest[..=close], number)?;
    let after = rest[close + 1..].trim_start();
    let after = after.strip_prefix("where ").ok_or_else(|| {
        ParseError::new(
            number,
            format!("expected `where` after the groups, as in `{example}`"),
        )
    })?;
    let mut tokens = after.split_whitespace();
    let product = qualified_identifier(tokens.next().unwrap_or(""), number, "constraint product")?;
    Ok((dimensions, product, tokens))
}

/// The source, terms and dimensions after `per` of a rule in the old
/// `require` and `skip` order, which only errors read now.
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
    if !rest.starts_with('[') {
        if let Some(error) = drop_in_require_order(rest, number) {
            return Err(error);
        }
    }
    let example = "drop [sub] where sessions count<2";
    let (dimensions, product, mut tokens) = parse_groups_where(rest, number, example, syntax)?;
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

/// The error for a `require` rule in its old order, source first and the
/// groups after `per`, with the rule rewritten, or `None` when the text
/// does not read as one.
fn require_in_old_order(rest: &str, number: usize) -> Option<ParseError> {
    let syntax = "expected `require [dimensions] where source`";
    let (product, terms, dimensions) = parse_require_parts(rest, number, syntax).ok()?;
    let mut rule = format!("require [{}] where {product}", dimensions.join(", "));
    if let Some(count) = terms.count {
        rule.push(' ');
        rule.push_str(&count.as_written());
    }
    if !terms.values.is_empty() {
        rule.push_str(" has ");
        rule.push_str(&value_terms(&owned_values(terms.values)));
    }
    Some(ParseError::new(
        number,
        format!("`require` names the groups first, then `where`, as `drop` does: write `{rule}`"),
    ))
}

/// The error for a `drop` rule written in `require`'s order, source first,
/// or `None` when the text does not read as one. A count keeps its
/// comparison; values could mean `has` or `missing`, so both are named.
fn drop_in_require_order(rest: &str, number: usize) -> Option<ParseError> {
    let syntax = "expected `drop [dimensions] where source`";
    let (product, terms, dimensions) = parse_require_parts(rest, number, syntax).ok()?;
    let start = format!("drop [{}] where {product}", dimensions.join(", "));
    let shape = "`drop` names the groups first, then `where`";
    let message = match (terms.count, terms.values.is_empty()) {
        (Some(count), true) => format!("{shape}: write `{start} {}`", count.as_written()),
        (None, false) => {
            let values = value_terms(&owned_values(terms.values));
            format!(
                "{shape}: write `{start} has {values}` to remove the groups that have them, \
                 or `{start} missing {values}` to remove the groups that lack one"
            )
        }
        _ => format!("{shape}, as in `drop [sub] where sessions count<2`"),
    };
    Some(ParseError::new(number, message))
}

/// Values as a rule writes them: `run=1,2 echo=1`.
fn value_terms(values: &BTreeMap<String, Vec<String>>) -> String {
    values
        .iter()
        .map(|(dimension, listed)| format!("{dimension}={}", listed.join(",")))
        .collect::<Vec<_>>()
        .join(" ")
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
