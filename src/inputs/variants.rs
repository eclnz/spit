//! Values of one dimension that differ only in letter case, such as store
//! `s07` in the sales and `S07` in the price list. SPIT compares values as
//! written, so the two are different stores; listing them together shows a
//! reader which sources are probably one thing under two spellings.

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
    pub excluded: bool,
}

/// The spellings of one value that differ only in letter case, sorted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaseVariants {
    pub dimension: String,
    pub spellings: Vec<Spelling>,
}

/// Each set of values of a dimension that differ only in letter case, among
/// the sources the inventory holds and the groups it removed, in dimension
/// then value order. A set where every spelling was removed is left out: the
/// reader has dealt with it.
///
/// This reads each source once, noting each distinct value of each of its
/// dimensions once, and compares only the distinct values.
pub fn case_variants(inventory: &SourceInventory) -> Vec<CaseVariants> {
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
        let values = folded
            .entry((dimension, value.to_ascii_lowercase()))
            .or_default();
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
            sets.push(CaseVariants {
                dimension: dimension.to_owned(),
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

    #[test]
    fn spellings_that_differ_only_in_case_are_listed_together() {
        let sets = case_variants(&inventory(
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
        let sets = case_variants(&inventory(&[("pricing", "S07")], &["s07"]));
        assert_eq!(sets.len(), 1);
        let spellings = &sets[0].spellings;
        assert!(!spellings[0].excluded && spellings[0].products == ["pricing"]);
        assert!(spellings[1].excluded && spellings[1].products.is_empty());
    }

    #[test]
    fn a_set_with_every_spelling_removed_is_left_out() {
        assert!(case_variants(&inventory(&[], &["s07", "S07"])).is_empty());
    }

    #[test]
    fn values_that_agree_in_case_or_differ_otherwise_are_not_variants() {
        assert!(case_variants(&inventory(
            &[("sales", "s07"), ("pricing", "s07"), ("sales", "s08")],
            &[]
        ))
        .is_empty());
    }
}
