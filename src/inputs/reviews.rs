//! Review files: the CSV file of each `review name from file` line of a
//! recipe, read into the verdict rows the plan judges artifacts by.

use std::collections::BTreeSet;
use std::fmt;
use std::io::ErrorKind;
use std::path::Path;

use rustc_hash::FxHashMap;

use crate::model::{DefinitionSubject, ResolveError};
use crate::model::{
    InputRules, Pipeline, PipelineIndex, ProductDef, ReviewFile, Verdict, VerdictRow,
};
use crate::parser::ParseError;
use crate::review::{attached_products, ReviewIndex};

/// Read the text of each file the `review ... from` lines of `rules` name,
/// relative to `folder`. A file that does not exist yet is left unread: its
/// review has no verdicts, which is no error.
pub(crate) fn read_review_files(rules: &mut InputRules, folder: &Path) -> Result<(), ParseError> {
    for review in &mut rules.reviews {
        match std::fs::read_to_string(folder.join(&review.file)) {
            Ok(text) => review.text = Some(text),
            Err(reason) if reason.kind() == ErrorKind::NotFound => {}
            Err(reason) => {
                return Err(ParseError::new(
                    review.line,
                    format!(
                        "cannot read `{}` for `review {}`: {reason}",
                        review.file, review.review
                    ),
                ))
            }
        }
    }
    Ok(())
}

/// A problem in a review file: the file, the line when it is on one, and
/// what is wrong.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewFileError {
    pub file: String,
    pub line: Option<usize>,
    pub problem: String,
}

impl fmt::Display for ReviewFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{} line {line}: {}", self.file, self.problem),
            None => write!(f, "{}: {}", self.file, self.problem),
        }
    }
}

/// The rows of every review file `rules` names that exists, review by
/// review, each checked against the products its review is attached to.
pub(crate) fn read_verdicts(
    pipeline: &Pipeline,
    rules: &InputRules,
) -> Result<Vec<VerdictRow>, ReviewFileError> {
    if rules.reviews.iter().all(|review| review.text.is_none()) {
        return Ok(Vec::new());
    }
    let index = PipelineIndex::new(pipeline);
    let attached = attached_products(&index, &ReviewIndex::new(&index));
    let mut verdicts = Vec::new();
    for file in &rules.reviews {
        let Some(text) = &file.text else {
            continue;
        };
        let products = attached
            .get(file.review.as_str())
            .map_or(&[][..], Vec::as_slice);
        let rows = rows_from_csv(text, file, products)?;
        check_conflicts(&rows, &file.file)?;
        verdicts.extend(rows);
    }
    Ok(verdicts)
}

/// The rows a CSV file holds: its header names each column, `verdict`,
/// `reason` or a dimension of every product `products` holds, and each row
/// after it gives a verdict to the artifacts its dimension cells name, an
/// empty cell leaving its column out.
fn rows_from_csv(
    text: &str,
    file: &ReviewFile,
    products: &[&ProductDef],
) -> Result<Vec<VerdictRow>, ReviewFileError> {
    let error = |line: Option<usize>, problem: String| ReviewFileError {
        file: file.file.clone(),
        line,
        problem,
    };
    let mut rows = csv_rows(text.strip_prefix('\u{feff}').unwrap_or(text))
        .map_err(|(line, problem)| error(Some(line), problem))?
        .into_iter();
    let Some((_, header)) = rows.next() else {
        return Err(error(
            None,
            "expected a header row, such as `sub,verdict,reason`".to_owned(),
        ));
    };
    let mut seen = BTreeSet::new();
    for column in header.iter().map(|column| column.trim()) {
        if !is_identifier(column) {
            return Err(error(
                Some(1),
                format!("column `{column}` must be `verdict`, `reason` or a dimension name"),
            ));
        }
        if !seen.insert(column) {
            return Err(error(Some(1), format!("column `{column}` appears twice")));
        }
        if matches!(column, "verdict" | "reason") {
            continue;
        }
        if let Some(product) = products
            .iter()
            .find(|product| !product.dimensions.iter().any(|name| name == column))
        {
            return Err(error(
                Some(1),
                format!(
                    "column `{column}` is not a dimension of `{}`, which review `{}` judges; a column is `verdict`, `reason` or a dimension of every product the review is attached to",
                    product.name, file.review
                ),
            ));
        }
    }
    if !seen.contains("verdict") {
        return Err(error(
            Some(1),
            "expected a `verdict` column, whose cells are `pass` or `fail`".to_owned(),
        ));
    }
    let mut verdicts = Vec::new();
    for (line, cells) in rows {
        if cells.len() != header.len() {
            return Err(error(
                Some(line),
                format!(
                    "expected {} cells, as the header has, found {}",
                    header.len(),
                    cells.len()
                ),
            ));
        }
        let mut verdict = None;
        let mut reason = None;
        let mut values = Vec::new();
        for (column, cell) in header.iter().map(|column| column.trim()).zip(&cells) {
            let cell = cell.trim();
            match column {
                "verdict" => {
                    verdict = Some(match cell {
                        "pass" => Verdict::Pass,
                        "fail" => Verdict::Fail,
                        _ => {
                            return Err(error(
                                Some(line),
                                format!("verdict `{cell}` must be `pass` or `fail`"),
                            ))
                        }
                    });
                }
                _ if cell.is_empty() => {}
                "reason" => reason = Some(cell.split_whitespace().collect::<Vec<_>>().join(" ")),
                dimension => {
                    if cell.chars().any(char::is_whitespace) {
                        return Err(error(
                            Some(line),
                            format!("the value of `{dimension}` must be one token, not `{cell}`"),
                        ));
                    }
                    values.push((dimension.to_owned(), cell.to_owned()));
                }
            }
        }
        if values.is_empty() {
            return Err(error(
                Some(line),
                "names no dimension value, so it judges nothing".to_owned(),
            ));
        }
        verdicts.push(VerdictRow {
            review: file.review.clone(),
            entities: values.into_iter().collect(),
            verdict: verdict.expect("a header with a verdict column gives every row a verdict"),
            reason,
            file: file.file.clone(),
            line,
        });
    }
    Ok(verdicts)
}

/// An error when two rows give conflicting verdicts to one artifact. Rows
/// judge a common artifact when they agree on every dimension both give,
/// so this needs no dataset, and a conflict is found whether or not the
/// artifact exists.
///
/// Rows are grouped by the dimensions they give; each pair of groups is
/// compared once, by hashing the first group's rows on the dimensions the
/// two share.
fn check_conflicts(rows: &[VerdictRow], file: &str) -> Result<(), ReviewFileError> {
    let mut groups: Vec<(Vec<&str>, Vec<&VerdictRow>)> = Vec::new();
    for row in rows {
        let dimensions: Vec<&str> = row
            .entities
            .iter()
            .map(|(dimension, _)| dimension)
            .collect();
        match groups.iter_mut().find(|(known, _)| *known == dimensions) {
            Some((_, members)) => members.push(row),
            None => groups.push((dimensions, vec![row])),
        }
    }
    let mut conflict: Option<(&VerdictRow, &VerdictRow)> = None;
    for (index, (left_dimensions, left)) in groups.iter().enumerate() {
        for (right_dimensions, right) in &groups[index..] {
            let shared: Vec<&str> = left_dimensions
                .iter()
                .copied()
                .filter(|dimension| right_dimensions.contains(dimension))
                .collect();
            let key = |row: &VerdictRow| -> Vec<Option<String>> {
                shared
                    .iter()
                    .map(|dimension| row.entities.get(dimension).map(str::to_owned))
                    .collect()
            };
            // The first row of each verdict, for each value of the shared
            // dimensions.
            let mut first: FxHashMap<Vec<Option<String>>, [Option<&VerdictRow>; 2]> =
                FxHashMap::default();
            for &row in left {
                first.entry(key(row)).or_default()[slot(row.verdict)].get_or_insert(row);
            }
            for &row in right {
                let Some(earlier) = first
                    .get(&key(row))
                    .and_then(|seen| seen[1 - slot(row.verdict)])
                else {
                    continue;
                };
                let later = row.line.max(earlier.line);
                if conflict.is_none_or(|(a, b)| later < a.line.max(b.line)) {
                    conflict = Some((row, earlier));
                }
            }
        }
    }
    let Some((row, other)) = conflict else {
        return Ok(());
    };
    let (row, other) = if row.line >= other.line {
        (row, other)
    } else {
        (other, row)
    };
    Err(ReviewFileError {
        file: file.to_owned(),
        line: Some(row.line),
        problem: format!(
            "verdict `{}` conflicts with `{}` on line {}, which judges some of the same artifacts",
            row.verdict.word(),
            other.verdict.word(),
            other.line
        ),
    })
}

fn slot(verdict: Verdict) -> usize {
    match verdict {
        Verdict::Pass => 0,
        Verdict::Fail => 1,
    }
}

/// Check each `review ... from` line against the pipeline without reading
/// any data: the review must be one the pipeline declares.
pub(crate) fn collect_review_errors(
    pipeline: &Pipeline,
    rules: &InputRules,
) -> Vec<(DefinitionSubject, ResolveError)> {
    rules
        .reviews
        .iter()
        .enumerate()
        .filter(|(_, file)| !pipeline.reviews.iter().any(|known| known.name == file.review))
        .map(|(index, file)| {
            let subject = DefinitionSubject::ReviewFile(index);
            let problem = ResolveError::InvalidDefinition {
                subject: subject.clone(),
                detail: format!(
                    "the pipeline declares no review `{}`; declare it with `review {}` and attach it with `@review({})`",
                    file.review, file.review, file.review
                ),
            };
            (subject, problem)
        })
        .collect()
}

/// A row of a CSV text: the line it starts on, and its fields.
type CsvRow = (usize, Vec<String>);

/// The rows of a CSV text, each with the line it starts on, as RFC 4180
/// reads them: fields separated by commas, a field in double quotes may hold
/// commas, line breaks and `""` for a quote. Blank lines are skipped.
fn csv_rows(text: &str) -> Result<Vec<CsvRow>, (usize, String)> {
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
        return Err((row_line, "a quoted field is not closed".to_owned()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProductDef;
    use crate::types::TypeExpr;

    fn file(text: &str) -> ReviewFile {
        ReviewFile {
            review: "looked_at".to_owned(),
            file: "qc/looked_at.csv".to_owned(),
            line: 3,
            text: Some(text.to_owned()),
        }
    }

    fn rows(text: &str) -> Result<Vec<VerdictRow>, ReviewFileError> {
        let product = ProductDef::new("t1w", TypeExpr::Unknown, ["sub", "ses"]);
        let file = file(text);
        let rows = rows_from_csv(text, &file, &[&product])?;
        check_conflicts(&rows, &file.file)?;
        Ok(rows)
    }

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
    fn a_review_file_holds_one_verdict_per_row() {
        let rows =
            rows("sub,ses,verdict,reason\n04,,fail,\"ghosting, frontal\"\n06,01,pass,\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].verdict, Verdict::Fail);
        assert_eq!(rows[0].reason.as_deref(), Some("ghosting, frontal"));
        assert_eq!(rows[0].entities.get("ses"), None);
        assert_eq!(rows[0].origin(), "qc/looked_at.csv line 2");
        assert_eq!(rows[1].entities.get("ses"), Some("01"));
        assert_eq!(rows[1].reason, None);
    }

    #[test]
    fn a_malformed_review_file_names_its_line() {
        let problem = |text: &str| rows(text).unwrap_err().to_string();
        assert!(problem("").contains("expected a header row"));
        assert!(problem("sub,reason\n04,x\n").contains("expected a `verdict` column"));
        assert!(
            problem("sub,verdict\n04,maybe\n").contains("qc/looked_at.csv line 2: verdict `maybe`")
        );
        assert!(problem("sub,verdict\n,pass\n").contains("line 2: names no dimension value"));
        assert!(problem("sub,verdict\n04\n").contains("line 2: expected 2 cells"));
        assert!(problem("sub name,verdict\n").contains("line 1: column `sub name`"));
        assert!(problem("sub,sub,verdict\n").contains("appears twice"));
        assert!(
            problem("run,verdict\n1,pass\n").contains("column `run` is not a dimension of `t1w`")
        );
        assert!(problem("sub,verdict\n\"04,pass\n").contains("not closed"));
    }

    #[test]
    fn rows_that_judge_one_artifact_must_agree() {
        let problem = |text: &str| rows(text).unwrap_err().to_string();
        assert!(problem("sub,ses,verdict\n04,01,pass\n04,01,fail\n")
            .contains("line 3: verdict `fail` conflicts with `pass` on line 2"));
        assert!(problem("sub,ses,verdict\n04,,fail\n04,01,pass\n")
            .contains("line 3: verdict `pass` conflicts with `fail` on line 2"));
        assert!(rows("sub,ses,verdict\n04,01,pass\n04,01,pass\n04,02,fail\n05,,fail\n").is_ok());
    }
}
