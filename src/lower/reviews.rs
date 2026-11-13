//! Lowering reviews: the `review` declarations of a pipeline, its `review:`
//! defaults, and the `review ... from` lines of a recipe.

use std::path::PathBuf;

use crate::model::{DefaultReviews, ReviewDef, ReviewFile, ReviewId};
use crate::parser::ParseError;
use crate::span::Place;

use super::PipelineBuilder;

impl PipelineBuilder {
    pub(crate) fn add_review(&mut self, review: ReviewDef, place: Place) {
        let id = ReviewId::at(self.pipeline.reviews.len());
        self.lines.reviews.push(id, place);
        self.pipeline.reviews.push(review);
    }

    /// Add a recipe's `review name from file` line. A review names its file
    /// once.
    pub(super) fn add_review_file(
        &mut self,
        review: &str,
        file: &str,
        place: &Place,
    ) -> Result<(), ParseError> {
        if let Some(earlier) = self
            .inputs
            .reviews
            .iter()
            .find(|earlier| earlier.review == review)
        {
            return Err(ParseError::new(
                place.line,
                format!(
                    "review `{review}` already has its file `{}` on line {}; a review names one file",
                    earlier.file, earlier.line
                ),
            )
            .within(place));
        }
        self.inputs.reviews.push(ReviewFile {
            review: review.to_owned(),
            file: file.to_owned(),
            path: PathBuf::from(file),
            line: place.line,
            text: None,
        });
        self.lines.review_files.push(place.clone());
        Ok(())
    }

    /// Add a `review:` list. One that repeats the list already given fails
    /// before it changes anything.
    pub(super) fn add_default_reviews(
        &mut self,
        stage: Option<&str>,
        reviews: &DefaultReviews,
        place: &Place,
    ) -> Result<(), ParseError> {
        let Self {
            pipeline, lines, ..
        } = self;
        let (slot, whose) = match stage {
            Some(stage) => (
                &mut pipeline
                    .stages
                    .iter_mut()
                    .find(|definition| definition.name == stage)
                    .expect("a stage is declared before its lines")
                    .reviews,
                format!(" for stage `{stage}`"),
            ),
            None => (&mut pipeline.default_reviews, String::new()),
        };
        if *slot != DefaultReviews::default() {
            return Err(ParseError::new(
                place.line,
                format!("duplicate `default review:` list{whose}; write the reviews in one line"),
            )
            .within(place));
        }
        *slot = reviews.clone();
        let placed = match stage {
            Some(stage) => lines.stage_reviews.entry(stage.to_owned()).or_default(),
            None => lines.default_reviews.get_or_insert_with(Place::default),
        };
        *placed = place.clone();
        Ok(())
    }
}
