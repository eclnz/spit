use rustc_hash::FxHashMap;

use crate::model::{ArtifactId, EntityBinding, Judged, Verdict, VerdictRow};

pub(super) struct Verdicts<'v> {
    rows: &'v [VerdictRow],
    reviews: FxHashMap<&'v str, Vec<Group<'v>>>,
}

struct Group<'v> {
    dimensions: Vec<&'v str>,
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

    pub(super) fn mark(&self, review: &str, entities: &EntityBinding, matched: &mut [bool]) {
        for index in self.covering(review, entities) {
            matched[index] = true;
        }
    }
}

pub(super) enum Block<'a> {
    Withheld,
    Review { review: &'a str, judged: Judged },
}

pub(super) struct Judging<'a> {
    pub(super) withheld: &'a [bool],
    pub(super) verdicts: &'a Verdicts<'a>,
    pub(super) ports: Vec<Vec<&'a str>>,
    pub(super) any_withheld: bool,
}

impl Judging<'_> {
    pub(super) fn active(&self) -> bool {
        self.any_withheld || self.ports.iter().any(|port| !port.is_empty())
    }

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
