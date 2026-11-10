//! Values of one dimension that are alike but not equal, as [`Likeness`]
//! defines it: store `s07` in the sales and `S07` in the price list, or take
//! `03` beside take `3`. SPIT compares values as written, so each pair is two
//! values; listing them together shows a reader which sources are probably
//! one thing under two spellings.

use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::model::SourceInventory;

/// One spelling of a value, and where it is found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spelling {
    pub value: String,
    /// The products with a remaining source that has this value, sorted.
    pub products: Vec<String>,
    /// An `exclude` rule removed a group with this value. A product's own
    /// artifact removed by name is not counted, since its group remains.
    /// The note marks a spelling `(excluded)` only when no source of it
    /// remains, so a spelling an `exclude` rule removed only some of, as
    /// `exclude [store=s01,week=w1]` does of `s01`, is listed with its
    /// remaining products and not marked.
    pub excluded: bool,
}

/// How two values can be alike while SPIT takes them as different.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Likeness {
    /// They differ only in ASCII letter case, as `s07` and `S07`. Letters
    /// outside ASCII are not folded: `é` and `É` are not alike.
    Case,
    /// Both are made only of ASCII digits and are equal as numbers, as `03`
    /// and `3`. A value with any other character, as `w03`, is never alike
    /// in this way.
    Number,
}

impl Likeness {
    /// The value every spelling alike in this way shares, or `None` when the
    /// value can't be alike in this way.
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

/// The spellings of one value that are alike in one way, sorted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Variants {
    pub dimension: String,
    pub likeness: Likeness,
    pub spellings: Vec<Spelling>,
}

/// Each set of values of a dimension that are alike as `likeness` says,
/// among the sources the inventory holds and the groups it removed, in
/// dimension then value order. A set where every spelling was removed is left
/// out: the reader has dealt with it.
///
/// This reads each source once, noting each distinct value of each of its
/// dimensions once, and compares only the distinct values.
pub fn variants(inventory: &SourceInventory, likeness: Likeness) -> Vec<Variants> {
    // The products of the remaining sources, by dimension and value.
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
        // The ASCII letters of a value fold even beside others that do not.
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
