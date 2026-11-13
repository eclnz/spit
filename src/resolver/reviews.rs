//! Judging reviewed artifacts: looking up the verdict a review's file gives
//! an artifact, and deciding whether a reader may use it.

use rustc_hash::FxHashMap;

use crate::model::{ArtifactId, EntityBinding, Judged, Verdict, VerdictRow};

/// The rows of a dataset's review files, looked up by an artifact's
/// dimensions.
///
/// A row gives values for some dimensions and covers every artifact that has
/// them. Rows of one review are grouped by the dimensions they give, so an
/// artifact is looked up once per group, by hashing its values.
pub(super) struct Verdicts<'v> {
    rows: &'v [VerdictRow],
    reviews: FxHashMap<&'v str, Vec<Group<'v>>>,
}

/// The rows of one review that give the same dimensions.
struct Group<'v> {
    dimensions: Vec<&'v str>,
    /// Each combination of values, in `dimensions` order, with the rows that
    /// give it.
    rows: FxHashMap<Vec<&'v str>, Vec<usize>>,
}

impl<'v> Verdicts<'v> {
    pub(super) fn new(rows: &'v [VerdictRow]) -> Self {
        let mut reviews: FxHashMap<&str, Vec<Group<'_>>> = FxHashMap::default();
        for (index, row) in rows.iter().enumerate() {
            let dimensions: Vec<&str> = row
                .entities
                .iter()
                .map(|(dimension, _)| dimension)
                .collect();
            let values: Vec<&str> = row.entities.iter().map(|(_, value)| value).collect();
            let groups = reviews.entry(row.review.as_str()).or_default();
            let position = groups
                .iter()
                .position(|group| group.dimensions == dimensions)
                .unwrap_or_else(|| {
                    groups.push(Group {
                        dimensions,
                        rows: FxHashMap::default(),
                    });
                    groups.len() - 1
                });
            groups[position].rows.entry(values).or_default().push(index);
        }
        Self { rows, reviews }
    }

    /// Every row of `review` that covers an artifact with `entities`.
    fn covering(&self, review: &str, entities: &EntityBinding) -> Vec<usize> {
        let mut covering = Vec::new();
        for group in self.reviews.get(review).into_iter().flatten() {
            let key: Option<Vec<&str>> = group
                .dimensions
                .iter()
                .map(|dimension| entities.get(dimension))
                .collect();
            if let Some(rows) = key.and_then(|key| group.rows.get(&key)) {
                covering.extend(rows);
            }
        }
        covering
    }

    /// How `review` judges an artifact with `entities`: `None` when a row
    /// passes it. Rows that cover one artifact agree, which the input stage
    /// checks, so the first decides.
    pub(super) fn judge(&self, review: &str, entities: &EntityBinding) -> Option<Judged> {
        let Some(&index) = self.covering(review, entities).first() else {
            return Some(Judged::Held);
        };
        let row = &self.rows[index];
        match row.verdict {
            Verdict::Pass => None,
            Verdict::Fail => Some(Judged::Rejected {
                reason: row.reason.clone(),
                origin: row.origin(),
                line: row.line,
            }),
        }
    }

    /// Mark in `matched` each row of `review` that covers an artifact with
    /// `entities`.
    pub(super) fn mark(&self, review: &str, entities: &EntityBinding, matched: &mut [bool]) {
        for index in self.covering(review, entities) {
            matched[index] = true;
        }
    }
}

/// Why a reader may not use an artifact.
pub(super) enum Block<'a> {
    /// The artifact is withheld from every reader: the job that would have
    /// made it was left out, or a review on its product failed or held it.
    Withheld,
    /// A review on the reader's input port failed or held it.
    Review { review: &'a str, judged: Judged },
}

/// What a step needs to decide whether its jobs may read an artifact.
pub(super) struct Judging<'a> {
    /// Whether each artifact is withheld from every reader, by id.
    pub(super) withheld: &'a [bool],
    pub(super) verdicts: &'a Verdicts<'a>,
    /// The reviews on each of the step's input ports, by port; empty when no
    /// review is declared.
    pub(super) ports: Vec<Vec<&'a str>>,
    /// Whether any artifact is withheld from every reader so far.
    pub(super) any_withheld: bool,
}

impl Judging<'_> {
    /// Whether any artifact the step reads can be kept from it.
    pub(super) fn active(&self) -> bool {
        self.any_withheld || self.ports.iter().any(|port| !port.is_empty())
    }

    /// Why the job reading input `port` may not use `artifact`, if it may
    /// not.
    pub(super) fn block(
        &self,
        artifact: ArtifactId,
        entities: &EntityBinding,
        port: usize,
    ) -> Option<Block<'_>> {
        if self.withheld[artifact.index()] {
            return Some(Block::Withheld);
        }
        self.ports.get(port)?.iter().find_map(|&review| {
            self.verdicts
                .judge(review, entities)
                .map(|judged| Block::Review { review, judged })
        })
    }
}
