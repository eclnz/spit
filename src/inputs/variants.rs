use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::model::SourceInventory;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spelling {
    pub value: String,
    pub products: Vec<String>,
    pub excluded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Likeness {
    Case,
    Number,
}

impl Likeness {
    fn key(self, value: &str) -> Option<String> {
        match self {
            Likeness::Case => Some(value.to_ascii_lowercase()),
            Likeness::Number => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                let trimmed = value.trim_start_matches('0');
                Some(if trimmed.is_empty() { "0" } else { trimmed }.to_owned())
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Variants {
    pub dimension: String,
    pub likeness: Likeness,
    pub spellings: Vec<Spelling>,
}

pub fn variants(inventory: &SourceInventory, likeness: Likeness) -> Vec<Variants> {
    let mut found: FxHashMap<(&str, &str), Vec<&str>> = FxHashMap::default();
    for record in &inventory.artifacts {
        for (dimension, value) in record.entities.iter() {
            let products = found.entry((dimension, value)).or_default();
            if !products.contains(&record.product.as_str()) {
                products.push(&record.product);
            }
        }
    }
    let mut removed: FxHashSet<(&str, &str)> = FxHashSet::default();
    for removal in &inventory.removed {
        if removal.product.is_some() {
            continue;
        }
        for (dimension, value) in removal.entities.iter() {
            removed.insert((dimension, value));
        }
    }
    let mut folded: BTreeMap<(&str, String), Vec<&str>> = BTreeMap::new();
    for &(dimension, value) in found.keys().chain(removed.iter()) {
        let Some(key) = likeness.key(value) else {
            continue;
        };
        let values = folded.entry((dimension, key)).or_default();
        if !values.contains(&value) {
            values.push(value);
        }
    }
    let mut sets = Vec::new();
    for ((dimension, _), mut values) in folded {
        if values.len() < 2 {
            continue;
        }
        values.sort_unstable();
        let spellings: Vec<Spelling> = values
            .into_iter()
            .map(|value| {
                let mut products: Vec<String> = found
                    .get(&(dimension, value))
                    .map(|products| products.iter().map(|&product| product.to_owned()).collect())
                    .unwrap_or_default();
                products.sort_unstable();
                Spelling {
                    value: value.to_owned(),
                    products,
                    excluded: removed.contains(&(dimension, value)),
                }
            })
            .collect();
        if spellings
            .iter()
            .any(|spelling| !spelling.products.is_empty())
        {
            sets.push(Variants {
                dimension: dimension.to_owned(),
                likeness,
                spellings,
            });
        }
    }
    sets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntityBinding, Removal, SourceRecord};

    fn record(product: &str, store: &str) -> SourceRecord {
        SourceRecord::new(product, EntityBinding::from_pairs([("store", store)]))
    }

    fn inventory(records: &[(&str, &str)], removed: &[&str]) -> SourceInventory {
        SourceInventory {
            artifacts: records
                .iter()
                .map(|(product, store)| record(product, store))
                .collect(),
            removed: removed
                .iter()
                .map(|store| Removal {
                    product: None,
                    entities: EntityBinding::from_pairs([("store", *store)]),
                    rule: format!("exclude [store={store}]"),
                    origin: None,
                    reason: None,
                    found: None,
                })
                .collect(),
            ..SourceInventory::default()
        }
    }

    fn case(inventory: SourceInventory) -> Vec<Variants> {
        variants(&inventory, Likeness::Case)
    }

    fn number(inventory: SourceInventory) -> Vec<Variants> {
        variants(&inventory, Likeness::Number)
    }

    #[test]
    fn spellings_that_differ_only_in_case_are_listed_together() {
        let sets = case(inventory(
            &[
                ("sales", "s07"),
                ("sales", "s07"),
                ("pricing", "S07"),
                ("sales", "s01"),
            ],
            &[],
        ));
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].dimension, "store");
        let spellings: Vec<_> = sets[0]
            .spellings
            .iter()
            .map(|spelling| (spelling.value.as_str(), spelling.products.clone()))
            .collect();
        assert_eq!(
            spellings,
            [
                ("S07", vec!["pricing".to_owned()]),
                ("s07", vec!["sales".to_owned()])
            ]
        );
    }

    #[test]
    fn a_removed_group_still_shows_beside_its_remaining_spelling() {
        let sets = case(inventory(&[("pricing", "S07")], &["s07"]));
        assert_eq!(sets.len(), 1);
        let spellings = &sets[0].spellings;
        assert!(!spellings[0].excluded && spellings[0].products == ["pricing"]);
        assert!(spellings[1].excluded && spellings[1].products.is_empty());
    }

    #[test]
    fn a_set_with_every_spelling_removed_is_left_out() {
        assert!(case(inventory(&[], &["s07", "S07"])).is_empty());
    }

    #[test]
    fn letters_outside_ascii_are_not_folded() {
        assert!(case(inventory(
            &[
                ("sales", "é1"),
                ("pricing", "É1"),
                ("sales", "ß"),
                ("pricing", "SS")
            ],
            &[]
        ))
        .is_empty());
        assert_eq!(
            case(inventory(&[("sales", "é1a"), ("pricing", "é1A")], &[])).len(),
            1
        );
    }

    #[test]
    fn values_that_agree_in_case_or_differ_otherwise_are_not_variants() {
        assert!(case(inventory(
            &[("sales", "s07"), ("pricing", "s07"), ("sales", "s08")],
            &[]
        ))
        .is_empty());
    }

    #[test]
    fn values_equal_as_numbers_are_listed_together() {
        let sets = number(inventory(
            &[
                ("sales", "03"),
                ("sales", "3"),
                ("sales", "4"),
                ("pricing", "003"),
            ],
            &[],
        ));
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].likeness, Likeness::Number);
        let values: Vec<_> = sets[0]
            .spellings
            .iter()
            .map(|spelling| spelling.value.as_str())
            .collect();
        assert_eq!(values, ["003", "03", "3"]);
    }

    #[test]
    fn zeros_alone_are_equal_as_numbers() {
        assert_eq!(
            number(inventory(&[("sales", "0"), ("sales", "00")], &[])).len(),
            1
        );
    }

    #[test]
    fn values_with_other_characters_are_not_numbers() {
        assert!(number(inventory(
            &[
                ("sales", "w03"),
                ("sales", "w3"),
                ("sales", "3.0"),
                ("sales", "3"),
                ("sales", "")
            ],
            &[]
        ))
        .is_empty());
    }

    #[test]
    fn values_equal_as_numbers_are_not_case_variants() {
        assert!(case(inventory(&[("sales", "03"), ("sales", "3")], &[])).is_empty());
    }
}
