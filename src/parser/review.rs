use crate::model::{CheckUse, ReviewDef};

use super::check::{check_uses, Clause};
use super::error::ReviewError;
use super::lexical::{comma_items, identifier, qualified_identifier};
use super::ParseError;

#[derive(Clone, Debug)]
pub(crate) enum ReviewLine {
    Declare(ReviewDef),
    File { review: String, file: String },
}

pub(super) fn parse_review(text: &str, number: usize) -> Result<ReviewLine, ParseError> {
    let text = text.trim();
    let end = text
        .find(|c: char| c.is_whitespace() || c == ':' || c == '(')
        .unwrap_or(text.len());
    let (name, rest) = text.split_at(end);
    if name.is_empty() {
        return Err(ParseError::of(number, ReviewError::ExpectedReviewForm).at_token(text));
    }
    let rest = rest.trim_start();
    if rest.starts_with(':') {
        return Err(ParseError::of(
            number,
            ReviewError::ReviewHasCommand {
                name: name.to_string(),
            },
        )
        .at_token(rest));
    }
    if rest.starts_with('(') {
        return Err(ParseError::of(
            number,
            ReviewError::ReviewHasParameters {
                name: name.to_string(),
            },
        )
        .at_token(rest));
    }
    if rest.is_empty() {
        let name = identifier(name, number, "review name")?;
        return Ok(ReviewLine::Declare(ReviewDef {
            name: name.to_owned(),
        }));
    }
    let name = qualified_identifier(name, number, "review name")?;
    let Some(file) = rest.strip_prefix("from") else {
        return Err(ParseError::of(number, ReviewError::ExpectedReviewForm).at_token(rest));
    };
    let file = file.trim();
    if file.is_empty() || file.contains(char::is_whitespace) {
        return Err(ParseError::of(
            number,
            ReviewError::ExpectedReviewFile {
                name: name.to_string(),
            },
        )
        .at_token(rest));
    }
    Ok(ReviewLine::File {
        review: name.to_owned(),
        file: file.to_owned(),
    })
}

fn review_uses(
    clause: &Clause<'_>,
    number: usize,
) -> Result<(Vec<String>, Vec<String>), ParseError> {
    let items = comma_items(clause.argument, number)?;
    if items.is_empty() {
        return Err(ParseError::of(number, ReviewError::EmptyReviewClause).at_token(clause.text));
    }
    review_list(items, number)
}

pub(super) fn parse_default_reviews(
    text: &str,
    number: usize,
) -> Result<(Vec<String>, Vec<String>), ParseError> {
    let items = comma_items(text, number)?;
    if items.is_empty() {
        return Err(ParseError::of(number, ReviewError::EmptyDefaultReviews));
    }
    review_list(items, number)
}

fn review_list(items: Vec<&str>, number: usize) -> Result<(Vec<String>, Vec<String>), ParseError> {
    let mut uses = Vec::new();
    let mut exempt = Vec::new();
    for item in items {
        let (written, opt_out) = match item.strip_prefix('!') {
            Some(rest) => (rest.trim_start(), true),
            None => (item, false),
        };
        if written.contains('(') {
            let bare = written.split('(').next().unwrap_or(written).trim();
            return Err(ParseError::of(
                number,
                ReviewError::ReviewHasArguments {
                    bare: bare.to_string(),
                },
            )
            .at_token(item));
        }
        let name = qualified_identifier(written, number, "review name")?.to_owned();
        if opt_out { &mut exempt } else { &mut uses }.push(name);
    }
    Ok((uses, exempt))
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
                let (uses, opted_out) = review_uses(clause, number)?;
                attached.reviews.extend(uses);
                attached.exempt_reviews.extend(opted_out);
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

pub(super) fn only_attachments(
    clauses: &[Clause<'_>],
    what: &str,
    number: usize,
) -> Result<Attached, ParseError> {
    attachments_opting_out_of(clauses, what, number, false)
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
    if let Some(review) = attached.exempt_reviews.first() {
        return Err(ParseError::of(
            number,
            ReviewError::OptOutOfDefaultReview {
                what: what.to_string(),
                review: review.to_string(),
            },
        )
        .at_token(review));
    }
    Ok(attached)
}

#[cfg(test)]
mod tests {
    use super::{attachments, parse_default_reviews, parse_review, ReviewLine};
    use crate::parser::check::split_clauses;
    use crate::parser::ClauseSite;

    #[test]
    fn a_review_line_declares_a_review_or_names_its_file() {
        assert!(matches!(
            parse_review("looked_at", 1).unwrap(),
            ReviewLine::Declare(review) if review.name == "looked_at"
        ));
        assert!(matches!(
            parse_review("looked_at from qc/looked_at.csv", 1).unwrap(),
            ReviewLine::File { review, file } if review == "looked_at" && file == "qc/looked_at.csv"
        ));
    }

    #[test]
    fn a_review_takes_no_command_and_no_parameters() {
        let error = parse_review("looked_at: look {@path}", 1).unwrap_err();
        assert!(error.message().contains("a review takes no command"));
        let error = parse_review("looked_at(3)", 1).unwrap_err();
        assert!(error.message().contains("a review takes no parameters"));
        assert!(parse_review("", 1).is_err());
        assert!(parse_review("looked_at from", 1).is_err());
        assert!(parse_review("looked_at from a b", 1).is_err());
        assert!(parse_review("looked_at beside x", 1).is_err());
    }

    #[test]
    fn a_default_list_separates_the_reviews_it_drops() {
        let (uses, exempt) = parse_default_reviews("a, b, !c", 1).unwrap();
        assert_eq!(uses, ["a", "b"]);
        assert_eq!(exempt, ["c"]);
        assert!(parse_default_reviews("", 1).is_err());
        assert!(parse_default_reviews("a(1)", 1).is_err());
    }

    #[test]
    fn clauses_attach_checks_and_reviews_together() {
        let (port, clauses) = split_clauses(
            "dwi: DWI @check(nonempty) @review(a, !b)",
            ClauseSite::OnPort,
            1,
        )
        .unwrap();
        assert_eq!(port, "dwi: DWI");
        let attached = attachments(&clauses, "an output", 1).unwrap();
        assert_eq!(attached.checks.len(), 1);
        assert_eq!(attached.reviews, ["a"]);
        assert_eq!(attached.exempt_reviews, ["b"]);
        let (_, clauses) = split_clauses("x @review()", ClauseSite::OnPort, 1).unwrap();
        assert!(attachments(&clauses, "an output", 1).is_err());
        let (_, clauses) = split_clauses("x @vary(a)", ClauseSite::OnPort, 1).unwrap();
        assert!(attachments(&clauses, "an output", 1).is_err());
    }
}
