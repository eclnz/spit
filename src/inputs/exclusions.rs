//! `exclude` rules: removing named artifacts and groups from a dataset,
//! written in a recipe or read from a CSV file, and recording what each
//! removed.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::Path;

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{
    near_reason, EntityBinding, Exclusion, InputRules, PipelineIndex, Removal, SourceInventory,
};
use crate::parser::ParseError;

/// Read each file the `exclude from` lines of `rules` name, relative to
/// `folder`, adding a rule for each row after the rules written inline.
pub(crate) fn read_exclusion_files(
    rules: &mut InputRules,
    folder: &Path,
) -> Result<(), ParseError> {
    for (file, line) in std::mem::take(&mut rules.exclusion_files) {
        let text = std::fs::read_to_string(folder.join(&file)).map_err(|reason| {
            ParseError::new(
                line,
                format!("cannot read `{file}` for `exclude from`: {reason}"),
            )
        })?;
        let rows = exclusions_from_csv(&text, &file)
            .map_err(|message| ParseError::new(line, format!("{file}: {message}")))?;
        rules.exclusions.extend(rows);
    }
    Ok(())
}

/// The rules a CSV file holds: its header names each column, `product` and
/// `reason` or a dimension, and each row after it is one rule, an empty
/// cell leaving its column out.
fn exclusions_from_csv(text: &str, file: &str) -> Result<Vec<Exclusion>, String> {
    let mut rows = csv_rows(text.strip_prefix('\u{feff}').unwrap_or(text))?.into_iter();
    let Some((_, header)) = rows.next() else {
        return Err("expected a header row, such as `product,sub,ses,run,reason`".to_owned());
    };
    let mut seen = BTreeSet::new();
    for column in &header {
        let column = column.trim();
        if !is_identifier(column) {
            return Err(format!(
                "line 1: column `{column}` must be `product`, `reason` or a dimension name"
            ));
        }
        if !seen.insert(column) {
            return Err(format!("line 1: column `{column}` appears twice"));
        }
    }
    let mut exclusions = Vec::new();
    for (line, cells) in rows {
        if cells.len() != header.len() {
            return Err(format!(
                "line {line}: expected {} cells, as the header has, found {}",
                header.len(),
                cells.len()
            ));
        }
        let mut product = None;
        let mut reason = None;
        let mut values = Vec::new();
        for (column, cell) in header.iter().map(|column| column.trim()).zip(&cells) {
            let cell = cell.trim();
            if cell.is_empty() {
                continue;
            }
            match column {
                "product" => {
                    if !is_qualified_identifier(cell) {
                        return Err(format!("line {line}: `{cell}` is not a source name"));
                    }
                    product = Some(cell.to_owned());
                }
                "reason" => reason = Some(cell.split_whitespace().collect::<Vec<_>>().join(" ")),
                dimension => {
                    if cell.chars().any(char::is_whitespace) {
                        return Err(format!(
                            "line {line}: the value of `{dimension}` must be one token, not `{cell}`"
                        ));
                    }
                    values.push((dimension.to_owned(), cell.to_owned()));
                }
            }
        }
        if product.is_none() && values.is_empty() {
            return Err(format!("line {line}: names no source and no value"));
        }
        exclusions.push(Exclusion {
            product,
            values,
            reason,
            origin: format!("{file} line {line}"),
        });
    }
    Ok(exclusions)
}

/// The rows of a CSV text, each with the line it starts on, as RFC 4180
/// reads them: fields separated by commas, a field in double quotes may hold
/// commas, line breaks and `""` for a quote. Blank lines are skipped.
fn csv_rows(text: &str) -> Result<Vec<(usize, Vec<String>)>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut line = 1;
    let mut row_line = 1;
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    // Whether the row so far holds anything, so a blank line is no row.
    let mut started = false;
    while let Some(character) = chars.next() {
        match (quoted, character) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, '\n') => {
                line += 1;
                field.push('\n');
            }
            (true, other) => field.push(other),
            (false, '"') if field.is_empty() => {
                quoted = true;
                started = true;
            }
            (false, ',') => {
                row.push(std::mem::take(&mut field));
                started = true;
            }
            (false, '\r') if chars.peek() == Some(&'\n') => {}
            (false, '\n') => {
                if started || !field.is_empty() {
                    row.push(std::mem::take(&mut field));
                    rows.push((row_line, std::mem::take(&mut row)));
                }
                started = false;
                line += 1;
                row_line = line;
            }
            (false, other) => {
                field.push(other);
                started = true;
            }
        }
    }
    if quoted {
        return Err(format!("line {row_line}: a quoted field is not closed"));
    }
    if started || !field.is_empty() {
        row.push(field);
        rows.push((row_line, row));
    }
    Ok(rows)
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn is_qualified_identifier(text: &str) -> bool {
    text.split("::").all(is_identifier)
}

/// Check each rule against the pipeline without reading any data: a rule's
/// product must be a source, and each dimension it names must be one of that
/// source's or, for a rule naming no source, of some source.
pub(crate) fn collect_exclusion_errors(
    products: &PipelineIndex<'_>,
    rules: &InputRules,
) -> Vec<(DefinitionSubject, ResolveError)> {
    let pipeline = products.pipeline;
    let mut errors = Vec::new();
    // The dimensions of every source, for the rules that name none.
    let all_dimensions: Vec<&str> = pipeline
        .products
        .iter()
        .filter(|product| products.is_source(&product.name))
        .flat_map(|product| product.dimensions.iter().map(String::as_str))
        .collect();
    for (index, rule) in rules.exclusions.iter().enumerate() {
        // A rule written in the recipe is placed at its line; a row of a
        // file is not, so its message says where it is.
        let at = if rule.origin.starts_with("line ") {
            String::new()
        } else {
            format!(" ({})", rule.origin)
        };
        let problem = |detail: String| ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Exclusion(index),
            detail: format!("`{rule}`{at}: {detail}"),
        };
        let dimensions: Cow<'_, [&str]> = match &rule.product {
            Some(name) => {
                if !products.is_source(name) {
                    errors.push((
                        DefinitionSubject::Exclusion(index),
                        problem(format!("`{name}` is not a source")),
                    ));
                    continue;
                }
                Cow::Owned(
                    pipeline
                        .products
                        .iter()
                        .filter(|product| &product.name == name)
                        .flat_map(|product| product.dimensions.iter().map(String::as_str))
                        .collect(),
                )
            }
            None => Cow::Borrowed(all_dimensions.as_slice()),
        };
        for (dimension, _) in &rule.values {
            if !dimensions.contains(&dimension.as_str()) {
                let owner = rule
                    .product
                    .as_ref()
                    .map_or_else(|| "any source".to_owned(), |name| format!("`{name}`"));
                errors.push((
                    DefinitionSubject::Exclusion(index),
                    problem(format!("`{dimension}` is not a dimension of {owner}")),
                ));
            }
        }
    }
    errors
}

/// Applies `exclude` rules to what the input stage finds, one artifact or
/// context at a time, and records what each removes.
pub(crate) struct Excluder<'a> {
    rules: &'a [Exclusion],
    matched: Vec<bool>,
    /// Whether a rule that names no source has recorded its group.
    recorded: Vec<bool>,
    removed: Vec<(usize, Removal)>,
    /// Values close to those each rule names, seen where it matched nothing.
    near: Vec<BTreeSet<(String, String)>>,
}

impl<'a> Excluder<'a> {
    pub(crate) fn new(rules: &'a [Exclusion]) -> Self {
        Self {
            rules,
            matched: vec![false; rules.len()],
            recorded: vec![false; rules.len()],
            removed: Vec::new(),
            near: vec![BTreeSet::new(); rules.len()],
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whether a rule removes the artifact of `product` with `entities`,
    /// without recording it.
    pub(crate) fn excludes(&self, product: &str, entities: &EntityBinding) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.matches(product, entities))
    }

    /// Whether a rule removes the artifact of `product` with `entities`,
    /// recording it with the first rule that does.
    pub(crate) fn artifact(&mut self, product: &str, entities: &EntityBinding) -> bool {
        let mut first = None;
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.matches(product, entities) {
                self.matched[index] = true;
                first.get_or_insert(index);
            } else if !self.matched[index] {
                note_near(&mut self.near[index], rule, Some(product), entities);
            }
        }
        let Some(index) = first else {
            return false;
        };
        self.record(index, Some(product), entities);
        true
    }

    /// Whether a rule that names no source removes the discovered context
    /// `binding`, recording it.
    pub(crate) fn context(&mut self, binding: &EntityBinding) -> bool {
        let mut first = None;
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.matches_context(binding) {
                self.matched[index] = true;
                first.get_or_insert(index);
            } else if !self.matched[index] {
                note_near(&mut self.near[index], rule, None, binding);
            }
        }
        let Some(index) = first else {
            return false;
        };
        self.record(index, None, binding);
        true
    }

    /// Remove every record and context of `inventory` a rule names.
    pub(crate) fn apply(&mut self, inventory: &mut SourceInventory) {
        if self.is_empty() {
            return;
        }
        inventory
            .artifacts
            .retain(|record| !self.artifact(&record.product, &record.entities));
        inventory.contexts.retain(|binding| !self.context(binding));
        for bindings in inventory.discovered.values_mut() {
            bindings.retain(|binding| !self.context(binding));
        }
    }

    /// A rule that names a source records each artifact it removes; one
    /// that names none records its group once.
    fn record(&mut self, index: usize, product: Option<&str>, entities: &EntityBinding) {
        let rule = &self.rules[index];
        let (product, entities) = match &rule.product {
            Some(_) => match product {
                Some(product) => (Some(product.to_owned()), entities.clone()),
                None => return,
            },
            None if self.recorded[index] => return,
            None => {
                self.recorded[index] = true;
                (None, rule.binding())
            }
        };
        self.removed.push((
            index,
            Removal {
                product,
                entities,
                rule: rule.to_string(),
                origin: Some(rule.origin.clone()),
                reason: rule.reason.clone(),
                found: None,
            },
        ));
    }

    /// What the rules removed, rule by rule, or, for the first rule that
    /// removed nothing, the error.
    pub(crate) fn finish(mut self) -> Result<Vec<Removal>, UnmatchedExclusion> {
        if let Some(index) = self.matched.iter().position(|matched| !matched) {
            let rule = &self.rules[index];
            let near: Vec<_> = self.near[index]
                .iter()
                .map(|(dimension, value)| format!("{dimension}={value}"))
                .collect();
            return Err(UnmatchedExclusion {
                rule: rule.to_string(),
                origin: rule.origin.clone(),
                near,
            });
        }
        self.removed
            .sort_by(|(left_rule, left), (right_rule, right)| {
                left_rule
                    .cmp(right_rule)
                    .then_with(|| left.product.cmp(&right.product))
                    .then_with(|| left.entities.cmp_in(&right.entities, &[]))
            });
        Ok(self
            .removed
            .into_iter()
            .map(|(_, removal)| removal)
            .collect())
    }
}

/// An `exclude` rule that matched nothing in the dataset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnmatchedExclusion {
    pub rule: String,
    pub origin: String,
    /// Values the dataset has that differ from the rule's only in letter case
    /// or leading zeros, as `sub=02`.
    pub near: Vec<String>,
}

/// Note each value of `entities` that is close to, but not, a value `rule`
/// names, where the rest of the rule could match.
fn note_near(
    near: &mut BTreeSet<(String, String)>,
    rule: &Exclusion,
    product: Option<&str>,
    entities: &EntityBinding,
) {
    if rule
        .product
        .as_deref()
        .is_some_and(|name| Some(name) != product)
    {
        return;
    }
    for (dimension, value) in &rule.values {
        if let Some(found) = entities.get(dimension) {
            if near_reason(found, value).is_some() {
                near.insert((dimension.to_owned(), found.to_owned()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_rows_follow_rfc_4180() {
        let rows = csv_rows("a,b\r\n\"x, y\",\"say \"\"hi\"\"\"\n\n\"two\nlines\",z\n").unwrap();
        assert_eq!(
            rows,
            [
                (1, vec!["a".into(), "b".into()]),
                (2, vec!["x, y".into(), "say \"hi\"".into()]),
                (4, vec!["two\nlines".into(), "z".into()]),
            ]
        );
        assert!(csv_rows("a,\"b\n").is_err());
    }

    #[test]
    fn a_csv_file_holds_one_rule_per_row() {
        let text = "product,sub,ses,run,reason\nbold,02,02,3,\"motion spike, volume 140\"\n,03,,,withdrew\n";
        let rules = exclusions_from_csv(text, "qc.csv").unwrap();
        assert_eq!(rules[0].to_string(), "exclude bold[sub=02,ses=02,run=3]");
        assert_eq!(rules[0].reason.as_deref(), Some("motion spike, volume 140"));
        assert_eq!(rules[0].origin, "qc.csv line 2");
        assert_eq!(rules[1].to_string(), "exclude [sub=03]");
        assert!(exclusions_from_csv("product,sub\n,\n", "qc.csv")
            .unwrap_err()
            .contains("line 2: names no source and no value"));
        assert!(exclusions_from_csv("product,sub name\n", "qc.csv").is_err());
        assert!(exclusions_from_csv("sub\n1,2\n", "qc.csv")
            .unwrap_err()
            .contains("expected 1 cells"));
    }

    #[test]
    fn near_values_differ_in_case_or_leading_zeros() {
        assert!(near_reason("S07", "s07").is_some());
        assert!(near_reason("02", "2").is_some());
        assert!(near_reason("03", "2").is_none());
        assert!(near_reason("run-2", "run-02").is_none());
    }
}
