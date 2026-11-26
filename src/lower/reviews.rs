use std::path::PathBuf;

use crate::model::{ReviewDef, ReviewFile, ReviewId};
use crate::parser::error::LowerError;
use crate::parser::ParseError;
use crate::span::Place;

use super::PipelineBuilder;

impl PipelineBuilder {
    pub(crate) fn add_review(&mut self, review: ReviewDef, place: Place) {
        let id = ReviewId::at(self.pipeline.reviews.len());
        self.lines.reviews.push(id, place);
        self.pipeline.reviews.push(review);
    }

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
            return Err(ParseError::of(
                place.line,
                LowerError::ReviewFileRepeated {
                    review: review.to_string(),
                    file: earlier.file.to_string(),
                    line: earlier.line,
                },
            )
            .within(place));
        }
        self.inputs.reviews.push(ReviewFile {
            review: review.to_owned(),
            file: file.to_owned(),
            path: PathBuf::from(file),
            line: place.line,
        });
        self.lines.review_files.push(place.clone());
        Ok(())
    }
}
