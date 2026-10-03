//! Names for suggested sources: a declared source a group fits, else a
//! word from its files, and `sidecars` for groups that share a stem.

use std::collections::BTreeSet;

use super::draft::{pieces, Draft};
use super::SuggestedSource;
use crate::inputs::pattern::match_pattern;
use crate::model::ProductDef;

/// Name each draft's source: a declared source it fits, else a word from
/// its file names, unique among the pipeline's products and each other.
/// Drafts no declared source takes that share a stem and dimensions, as an
/// image and its JSON do, become one `sidecars` group. Also returns the
/// declared sources no draft fits.
pub(super) fn name_sources(
    drafts: Vec<Draft>,
    declared: &[&ProductDef],
    taken: &BTreeSet<&str>,
    files: &[String],
) -> (Vec<SuggestedSource>, Vec<String>) {
    let mut chosen: Vec<(Draft, String, Option<&ProductDef>)> = drafts
        .into_iter()
        .map(|draft| {
            let word = draft
                .suffix
                .clone()
                .or_else(|| draft.name_words.last().cloned())
                .or_else(|| draft.folder_words.last().cloned())
                .unwrap_or_else(|| "files".to_owned());
            let name = identifier(&word);
            (draft, name, None)
        })
        .collect();
    // A declared source takes the one draft named for it, else the one whose
    // name starts with its own or its with the draft's, as `t1` and `t1w`,
    // else the one whose dimensions are its own.
    let mut unfitted = Vec::new();
    for product in declared {
        let wanted: BTreeSet<_> = product.dimensions.iter().cloned().collect();
        let fits = |test: usize, draft: &Draft, name: &str| match test {
            0 => name == product.name,
            1 => name.starts_with(&product.name) || product.name.starts_with(name),
            _ => draft.names().into_iter().collect::<BTreeSet<_>>() == wanted,
        };
        let fitting = (0..3).find_map(|test| {
            let fitting: Vec<_> = chosen
                .iter()
                .enumerate()
                .filter(|(_, (draft, name, owner))| owner.is_none() && fits(test, draft, name))
                .map(|(index, _)| index)
                .collect();
            (!fitting.is_empty()).then_some(fitting)
        });
        match fitting.as_deref() {
            Some(&[index]) => chosen[index].2 = Some(*product),
            _ => unfitted.push(product.name.clone()),
        }
    }
    let mut used: BTreeSet<String> = taken.iter().map(|name| (*name).to_owned()).collect();
    used.extend(declared.iter().map(|product| product.name.clone()));
    // Drafts by stem and dimensions, in the order they came, so that a
    // group of sidecars is suggested where its first member would be.
    let mut groups: Vec<Vec<(Draft, String, Option<&ProductDef>)>> = Vec::new();
    for entry in chosen {
        let together = groups.iter_mut().find(|group| {
            let (first, _, owner) = &group[0];
            owner.is_none()
                && entry.2.is_none()
                && first.stem() == entry.0.stem()
                && first.names() == entry.0.names()
        });
        match together {
            Some(group) => group.push(entry),
            None => groups.push(vec![entry]),
        }
    }
    let mut sources = Vec::new();
    for mut group in groups {
        let all: BTreeSet<&String> = group
            .iter()
            .flat_map(|(draft, _, _)| &draft.files)
            .collect();
        let overlaps = group
            .iter()
            .map(|(draft, _, _)| {
                let pieces = pieces(&draft.rule).unwrap_or_default();
                files
                    .iter()
                    .filter(|file| !all.contains(file) && match_pattern(&pieces, file).is_some())
                    .count()
            })
            .sum();
        let count = group.iter().map(|(draft, _, _)| draft.files.len()).sum();
        let sidecars = group.len() > 1;
        let (draft, name, product) = &mut group[0];
        let name = match product {
            Some(product) => {
                take_declared_names(draft, product);
                product.name.clone()
            }
            None if sidecars => unique_name(name, "", &mut used),
            None => unique_name(name, &draft.extension, &mut used),
        };
        let (draft, _, product) = &group[0];
        let members = if sidecars {
            group
                .iter()
                .map(|(member, _, _)| {
                    let suffix = identifier(member.extension.trim_start_matches('.'));
                    let member_name = unique_name(&format!("{name}_{suffix}"), "", &mut used);
                    (member_name, member.extension.clone())
                })
                .collect()
        } else {
            Vec::new()
        };
        let unnamed = draft
            .names()
            .into_iter()
            .filter(|name| is_placeholder_name(name) && product.is_none())
            .collect();
        sources.push(SuggestedSource {
            name,
            dimensions: draft.names(),
            unnamed,
            rule: if members.is_empty() {
                draft.rule.clone()
            } else {
                draft.stem()
            },
            files: count,
            example: draft.files[0].clone(),
            declared: product.map(|product| product.dimensions.clone()),
            overlaps,
            members,
        });
    }
    (sources, unfitted)
}

/// Whether `name` is one `name_unnamed` gave.
fn is_placeholder_name(name: &str) -> bool {
    name.strip_prefix("dim")
        .is_some_and(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
}

/// Give a draft for a declared source that source's dimension names, where
/// the two have as many: names the path gives stay, and the dimensions no
/// word names take the declared names left, in order.
fn take_declared_names(draft: &mut Draft, product: &ProductDef) {
    if draft.dimensions.len() != product.dimensions.len() {
        return;
    }
    let names = draft.names();
    let named: Vec<_> = names
        .iter()
        .filter(|name| !is_placeholder_name(name))
        .collect();
    if !named.iter().all(|name| product.dimensions.contains(name)) {
        return;
    }
    let mut left = product
        .dimensions
        .iter()
        .filter(|name| !named.contains(name));
    for dimension in &mut draft.dimensions {
        if dimension.name.as_deref().is_some_and(is_placeholder_name) {
            dimension.name = left.next().cloned();
        }
    }
    draft.rule = draft.render();
}

/// `word` as a product name: lowercase letters, digits and `_`, starting
/// with a letter.
fn identifier(word: &str) -> String {
    let mut name: String = word
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name.insert_str(0, "files_");
    }
    name
}

/// `name`, or with the extension's last part then a number added, so that
/// it is in `used` once.
fn unique_name(name: &str, extension: &str, used: &mut BTreeSet<String>) -> String {
    let mut candidates = vec![name.to_owned()];
    if let Some(last) = extension.rsplit('.').next().filter(|last| !last.is_empty()) {
        candidates.push(format!("{name}_{}", identifier(last)));
    }
    let mut number = 2;
    loop {
        if let Some(found) = candidates
            .iter()
            .find(|candidate| !used.contains(*candidate))
        {
            used.insert(found.clone());
            return found.clone();
        }
        candidates = vec![format!("{name}_{number}")];
        number += 1;
    }
}
