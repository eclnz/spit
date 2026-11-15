//! A recipe's coverage rules: `require` and conditional `exclude`, which name their
//! groups first, then `where` and the source they count, and the errors for
//! a rule written in another order, which say how to write it.

use std::collections::BTreeMap;

use crate::model::{CountRequirement, CoverageAction, CoverageRule};

use super::error::{RuleError, RuleForm};
use super::keyword::Keyword;
use super::lexical::{comma_items, identifier, qualified_identifier};
use super::ParseError;

/// Parse a `require` or conditional `exclude` rule, `rest` being the text
/// after its `keyword`.
pub(super) fn parse_coverage_rule(
    keyword: Keyword,
    rest: &str,
    number: usize,
) -> Result<CoverageRule, ParseError> {
    let rest = rest.trim_start();
    match keyword {
        Keyword::Exclude => parse_drop(rest, number),
        Keyword::Drop => Err(ParseError::of(
            number,
            RuleError::DropReplaced {
                rest: rest.to_string(),
            },
        )),
        Keyword::Skip => Err(skip_replaced(rest, number)),
        _ => parse_require(rest, number, RuleForm::Require),
    }
}

/// Parse the text after `require`: `[dimensions] where source`, then a
/// count, `has dimension=value,...`, or both, in that order.
fn parse_require(rest: &str, number: usize, form: RuleForm) -> Result<CoverageRule, ParseError> {
    let rest = rest.trim_start();
    if !rest.starts_with('[') {
        return Err(require_with_per(rest, number)
            .unwrap_or_else(|| ParseError::of(number, RuleError::ExpectedForm { form })));
    }
    let example = "require [sub, ses] where t1w count=1";
    let (dimensions, product, tokens) = parse_groups_where(rest, number, example, form)?;
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
            .ok_or_else(|| {
                ParseError::of(number, RuleError::ExpectedForm { form }).at_token(token)
            })?;
        rule.count = Some(count);
        rest = after;
    }
    match rest.split_first() {
        None if rule.count.is_none() => {
            return Err(ParseError::of(number, RuleError::ExpectedForm { form }))
        }
        None => {}
        Some((&"has", values)) => {
            if values.is_empty() {
                return Err(ParseError::of(number, RuleError::ExpectedValuesAfterHas));
            }
            let terms = parse_rule_terms(values.iter().copied(), number, form)?;
            if terms.count.is_some() {
                return Err(ParseError::of(number, RuleError::CountBeforeHas));
            }
            rule.values = owned_values(terms.values);
        }
        Some((token, _)) if token.contains('=') => {
            return Err(ParseError::of(
                number,
                RuleError::ValuesFollowHas {
                    token: token.to_string(),
                },
            )
            .at_token(token));
        }
        Some((token, _)) => {
            return Err(ParseError::of(number, RuleError::ExpectedForm { form }).at_token(token))
        }
    }
    Ok(rule)
}

/// The groups, `where`, and source that begin a conditional `exclude` or `require` rule,
/// with the tokens after the source. `example` shows the shape when `where`
/// is missing.
fn parse_groups_where<'a>(
    rest: &'a str,
    number: usize,
    example: &str,
    form: RuleForm,
) -> Result<(Vec<&'a str>, &'a str, std::str::SplitWhitespace<'a>), ParseError> {
    if !rest.starts_with('[') {
        return Err(ParseError::of(number, RuleError::ExpectedForm { form }));
    }
    // Without a `]`, the dimensions' own check says so, from the `[`.
    let close = match rest.find(']') {
        Some(close) => close,
        None => {
            return Err(parse_group_dimensions(rest, number)
                .err()
                .unwrap_or_else(|| ParseError::of(number, RuleError::ExpectedForm { form })))
        }
    };
    let dimensions = parse_group_dimensions(&rest[..=close], number)?;
    let after = rest[close + 1..].trim_start();
    let after = after.strip_prefix("where ").ok_or_else(|| {
        ParseError::of(
            number,
            RuleError::ExpectedWhereAfterGroups {
                example: example.to_string(),
            },
        )
    })?;
    let target_end = match after.strip_prefix("contexts[") {
        Some(_) => after
            .find(']')
            .map(|end| end + 1)
            .ok_or_else(|| ParseError::of(number, RuleError::UnclosedContextDimensions))?,
        None => after.find(char::is_whitespace).unwrap_or(after.len()),
    };
    let (target, terms) = after.split_at(target_end);
    let product = if let Some(dimensions) = target.strip_prefix("contexts[") {
        let dimensions = dimensions.strip_suffix(']').ok_or_else(|| {
            ParseError::of(number, RuleError::UnclosedContextDimensions).at_token(target)
        })?;
        let dimensions = comma_items(dimensions, number)?;
        let mut seen = std::collections::BTreeSet::new();
        for dimension in dimensions {
            identifier(dimension, number, "context dimension")?;
            if !seen.insert(dimension) {
                return Err(ParseError::of(number, RuleError::ContextDimensionsRepeat));
            }
        }
        target
    } else {
        qualified_identifier(target, number, "constraint product")?
    };
    Ok((dimensions, product, terms.split_whitespace()))
}

/// The source, terms and dimensions after `per` of a rule written in the
/// `require ... per [...]` order, which is read only to say what to write
/// instead.
fn parse_require_parts<'a>(
    rest: &'a str,
    number: usize,
    form: RuleForm,
) -> Result<(&'a str, RuleTerms<'a>, Vec<&'a str>), ParseError> {
    let (subject, dimensions) = rest
        .split_once(" per ")
        .ok_or_else(|| ParseError::of(number, RuleError::ExpectedForm { form }))?;
    let mut parts = subject.split_whitespace();
    let product = qualified_identifier(parts.next().unwrap_or(""), number, "constraint product")?;
    let terms = parse_rule_terms(parts, number, form)?;
    let dimensions = parse_group_dimensions(dimensions, number)?;
    Ok((product, terms, dimensions))
}

/// Parse the text after conditional `exclude`: `[dimensions] where source condition`, the
/// condition one of `count<2` (any comparison), `missing dimension=value,...`
/// or `has dimension=value,...`.
fn parse_drop(rest: &str, number: usize) -> Result<CoverageRule, ParseError> {
    let form = RuleForm::Exclude;
    let rest = rest.trim_start();
    if !rest.starts_with('[') {
        if let Some(error) = drop_in_require_order(rest, number) {
            return Err(error);
        }
    }
    let example = "exclude [sub] where sessions count<2";
    let (dimensions, product, mut tokens) = parse_groups_where(rest, number, example, form)?;
    let mut rule = CoverageRule::new(product, &dimensions, CountRequirement::AtLeast(1));
    rule.action = CoverageAction::Drop;
    rule.count = None;
    let condition = tokens
        .next()
        .ok_or_else(|| ParseError::of(number, RuleError::ExpectedForm { form }))?;
    let listed: Vec<&str> = tokens.collect();
    match condition {
        "missing" | "has" => {
            if listed.is_empty() {
                return Err(ParseError::of(
                    number,
                    RuleError::ExpectedValuesAfterCondition {
                        condition: condition.to_string(),
                    },
                ));
            }
            let values = owned_values(parse_rule_terms(listed.into_iter(), number, form)?.values);
            if condition == "missing" {
                rule.values = values;
            } else {
                rule.has = values;
            }
        }
        token => {
            let count = parse_count_term(token, number)
                .transpose()?
                .ok_or_else(|| {
                    ParseError::of(number, RuleError::ExpectedForm { form }).at_token(token)
                })?;
            if let Some(extra) = listed.first() {
                return Err(
                    ParseError::of(number, RuleError::ExcludeNeedsOneCondition).at_token(extra)
                );
            }
            rule.count = Some(count);
        }
    }
    Ok(rule)
}

/// The error for a `require` rule written source first, with the groups
/// after `per`, giving the rule rewritten, or `None` when the text does not
/// read as one.
fn require_with_per(rest: &str, number: usize) -> Option<ParseError> {
    let form = RuleForm::RequireGroups;
    let (product, terms, dimensions) = parse_require_parts(rest, number, form).ok()?;
    let mut rule = format!("require [{}] where {product}", dimensions.join(", "));
    if let Some(count) = terms.count {
        rule.push(' ');
        rule.push_str(&count.as_written());
    }
    if !terms.values.is_empty() {
        rule.push_str(" has ");
        rule.push_str(&value_terms(&owned_values(terms.values)));
    }
    Some(ParseError::of(
        number,
        RuleError::RequireGroupsFirst {
            rule: rule.to_string(),
        },
    ))
}

/// The error for a conditional `exclude` rule written source first,
/// or `None` when the text does not read as one. A count keeps its
/// comparison; values could mean `has` or `missing`, so both are named.
fn drop_in_require_order(rest: &str, number: usize) -> Option<ParseError> {
    let form = RuleForm::ExcludeGroups;
    let (product, terms, dimensions) = parse_require_parts(rest, number, form).ok()?;
    let start = format!("exclude [{}] where {product}", dimensions.join(", "));
    let error = match (terms.count, terms.values.is_empty()) {
        (Some(count), true) => RuleError::ExcludeGroupsFirstCount {
            start,
            count: count.as_written(),
        },
        (None, false) => RuleError::ExcludeGroupsFirstValues {
            start,
            values: value_terms(&owned_values(terms.values)),
        },
        _ => RuleError::ExcludeGroupsFirst,
    };
    Some(ParseError::of(number, error))
}

/// Values as a rule writes them: `run=1,2 echo=1`.
fn value_terms(values: &BTreeMap<String, Vec<String>>) -> String {
    values
        .iter()
        .map(|(dimension, listed)| format!("{dimension}={}", listed.join(",")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The error for a `skip` rule, with the conditional `exclude` rule that
/// removes the same groups when the `skip` rule reads cleanly.
fn skip_replaced(rest: &str, number: usize) -> ParseError {
    let old = RuleForm::Skip;
    let suggestion =
        parse_require_parts(rest, number, old)
            .ok()
            .map(|(product, terms, dimensions)| {
                let groups = dimensions.join(", ");
                let mut lines = Vec::new();
                if let Some(count) = terms.count {
                    lines.push(format!(
                        "exclude [{groups}] where {product} {}",
                        count.negated().as_written()
                    ));
                }
                for (dimension, values) in &terms.values {
                    lines.push(format!(
                        "exclude [{groups}] where {product} missing {dimension}={}",
                        values.join(",")
                    ));
                }
                lines.join("` and `")
            });
    let error = match suggestion {
        Some(rule) => RuleError::SkipReplaced { rule },
        None => RuleError::SkipReplacedWithoutRule,
    };
    ParseError::of(number, error)
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
    form: RuleForm,
) -> Result<RuleTerms<'a>, ParseError> {
    let mut count = None;
    let mut values = BTreeMap::new();
    for token in terms {
        if let Some(parsed) = parse_count_term(token, number) {
            if count.replace(parsed?).is_some() {
                return Err(ParseError::of(number, RuleError::RuleTakesCount).at_token(token));
            }
        } else if let Some((dimension, listed)) = token.split_once('=') {
            let dimension = identifier(dimension, number, "required dimension")?;
            let listed: Vec<_> = listed.split(',').collect();
            if listed.iter().any(|value| value.is_empty()) {
                return Err(ParseError::of(number, RuleError::RequiredValuesEmpty).at_token(token));
            }
            if values.insert(dimension.to_owned(), listed).is_some() {
                return Err(ParseError::of(
                    number,
                    RuleError::DuplicateRequiredDimension {
                        dimension: dimension.to_string(),
                    },
                )
                .at_token(token));
            }
        } else {
            return Err(ParseError::of(number, RuleError::ExpectedForm { form }).at_token(token));
        }
    }
    if count.is_none() && values.is_empty() {
        return Err(ParseError::of(number, RuleError::ExpectedForm { form }));
    }
    Ok(RuleTerms { count, values })
}

/// The `[dimension, ...]` a rule groups by.
fn parse_group_dimensions(dimensions: &str, number: usize) -> Result<Vec<&str>, ParseError> {
    let bracketed = dimensions.trim();
    let dimensions = bracketed.strip_prefix('[').ok_or_else(|| {
        ParseError::of(number, RuleError::ExpectedOpenBracket).at_token(bracketed)
    })?;
    let dimensions = dimensions.strip_suffix(']').ok_or_else(|| {
        ParseError::of(number, RuleError::UnclosedConstraintDimensions).at_token(bracketed)
    })?;
    let dimensions = comma_items(dimensions, number)?;
    for dimension in &dimensions {
        identifier(dimension, number, "constraint dimension")?;
    }
    Ok(dimensions)
}

fn parse_count(value: &str, number: usize) -> Result<usize, ParseError> {
    value
        .parse()
        .map_err(|_| ParseError::of(number, RuleError::CountNotInteger).at_token(value))
}
