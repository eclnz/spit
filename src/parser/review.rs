use crate::model::{CheckUse, Port, ReviewUse};

use super::check::{check_uses, Clause};
use super::error::ReviewError;
use super::lexical::{comma_items, qualified_identifier};
use super::ParseError;

#[derive(Clone, Debug)]
pub(crate) enum ReviewLine {
    File { review: String, file: String },
}

pub(super) fn parse_review(text: &str, number: usize) -> Result<ReviewLine, ParseError> {
    let text = text.trim();
    let Some((name, file)) = text.split_once(char::is_whitespace) else {
        return Err(ParseError::of(number, ReviewError::ExpectedReviewForm).at_token(text));
    };
    let name = qualified_identifier(name, number, "reviewed product")?;
    let Some(file) = file
        .trim_start()
        .strip_prefix("from")
        .filter(|rest| rest.starts_with(char::is_whitespace))
    else {
        return Err(ParseError::of(number, ReviewError::ExpectedReviewForm).at_token(text));
    };
    let file = file.trim();
    if file.is_empty() || file.contains(char::is_whitespace) {
        return Err(ParseError::of(
            number,
            ReviewError::ExpectedReviewFile {
                name: name.to_string(),
            },
        )
        .at_token(file));
    }
    Ok(ReviewLine::File {
        review: name.to_owned(),
        file: file.to_owned(),
    })
}

pub(super) fn call_reviews(
    clause: &Clause<'_>,
    outputs: &[String],
    number: usize,
) -> Result<Vec<ReviewUse>, ParseError> {
    let evidence = comma_items(clause.argument, number)?;
    if clause.text != "review" && evidence.is_empty() {
        return Err(ParseError::of(number, ReviewError::EmptyReviewClause).at_token(clause.text));
    }
    let mut seen = rustc_hash::FxHashSet::default();
    let mut products = Vec::with_capacity(evidence.len());
    for item in evidence {
        let item = qualified_identifier(item, number, "evidence product")?;
        if !seen.insert(item) {
            return Err(ParseError::of(number, ReviewError::DuplicateEvidence).at_token(item));
        }
        products.push(item.to_owned());
    }
    Ok((0..outputs.len())
        .map(|port| ReviewUse {
            port: Port::Output(port),
            name: None,
            evidence: products.clone(),
        })
        .collect())
}

#[derive(Default)]
pub(super) struct Attached {
    pub(super) checks: Vec<CheckUse>,
    pub(super) exempt: Vec<CheckUse>,
    pub(super) reviews: Vec<String>,
    pub(super) exempt_reviews: Vec<String>,
}

pub(super) fn is_attachment(clause: &Clause<'_>) -> bool {
    matches!(clause.keyword, "check" | "screen" | "review")
}

pub(super) fn attachments(
    clauses: &[Clause<'_>],
    what: &str,
    number: usize,
) -> Result<Attached, ParseError> {
    let mut attached = Attached::default();
    for clause in clauses {
        match clause.keyword {
            "check" | "screen" => {
                let (mut uses, mut opted_out) = check_uses(clause, number)?;
                if clause.keyword == "screen" {
                    for used in uses.iter_mut().chain(&mut opted_out) {
                        used.kind = crate::model::CheckKind::Screen;
                    }
                }
                attached.checks.extend(uses);
                attached.exempt.extend(opted_out);
            }
            "review" => {
                return Err(
                    ParseError::of(number, ReviewError::ReviewAfterCall).at_token(clause.text)
                );
            }
            other => {
                return Err(ParseError::of(
                    number,
                    ReviewError::ClauseNotAllowed {
                        what: what.to_string(),
                        other: other.to_string(),
                    },
                )
                .at_token(clause.text))
            }
        }
    }
    Ok(attached)
}

pub(super) fn input_attachments(
    clauses: &[Clause<'_>],
    what: &str,
    number: usize,
) -> Result<Vec<CheckUse>, ParseError> {
    if let Some(clause) = clauses.iter().find(|clause| clause.keyword == "review") {
        return Err(ParseError::of(number, ReviewError::ReviewOnInput).at_token(clause.text));
    }
    Ok(attachments_opting_out_of(clauses, what, number, false)?.checks)
}

pub(super) fn source_attachments(
    clauses: &[Clause<'_>],
    number: usize,
) -> Result<Attached, ParseError> {
    attachments_opting_out_of(clauses, "a source", number, true)
}

fn attachments_opting_out_of(
    clauses: &[Clause<'_>],
    what: &str,
    number: usize,
    checks_may_opt_out: bool,
) -> Result<Attached, ParseError> {
    let attached = attachments(clauses, what, number)?;
    if let Some(used) = attached.exempt.first().filter(|_| !checks_may_opt_out) {
        return Err(ParseError::of(
            number,
            ReviewError::OptOutOfDefaultCheck {
                what: what.to_string(),
                used: used.to_string(),
            },
        )
        .at_token(&used.check));
    }
    Ok(attached)
}

#[cfg(test)]
mod tests {
    use super::{parse_review, ReviewLine};

    #[test]
    fn a_recipe_review_names_a_products_verdict_file() {
        assert!(matches!(
            parse_review("fit::image from qc/image.csv", 1).unwrap(),
            ReviewLine::File { review, file } if review == "fit::image" && file == "qc/image.csv"
        ));
        for text in [
            "",
            "image",
            "image from",
            "image fromfile",
            "image from a b",
        ] {
            assert!(parse_review(text, 1).is_err(), "{text}");
        }
    }
}
