use rustc_hash::FxHashSet;

use crate::model::{identity, Preference, Removal, SourceRecord};
use crate::template::{error, PathError};

pub(super) fn one_record_each(
    records: &mut Vec<SourceRecord>,
    preferences: &[Preference],
    folders: &FxHashSet<&str>,
    removed: &mut Vec<Removal>,
) -> Result<(), PathError> {
    let mut chose = vec![false; preferences.len()];
    let mut dropped = vec![false; records.len()];
    let mut duplicates = Vec::new();
    let mut start = 0;
    while start < records.len() {
        let first = &records[start];
        let end = start
            + records[start..]
                .iter()
                .take_while(|record| {
                    record.product == first.product && record.entities == first.entities
                })
                .count();
        if end - start > 1 {
            let mut kept: Vec<usize> = (start..end).collect();
            for (index, preference) in preferences.iter().enumerate() {
                let (covered, others): (Vec<usize>, Vec<usize>) =
                    kept.iter().partition(|&&record| {
                        records[record]
                            .path
                            .as_deref()
                            .is_some_and(|path| preference.path.covers(path))
                    });
                if covered.is_empty() || others.is_empty() {
                    continue;
                }
                chose[index] = true;
                for record in others {
                    dropped[record] = true;
                    removed.push(Removal {
                        product: Some(first.product.clone()),
                        entities: first.entities.clone(),
                        rule: preference.to_string(),
                        origin: Some(preference.origin.clone()),
                        reason: preference.reason.clone(),
                        found: None,
                    });
                }
                kept = covered;
            }
            if kept.len() > 1 {
                duplicates.push(named(&records[start], &kept, records, folders));
            }
        }
        start = end;
    }
    if !duplicates.is_empty() {
        let each = if duplicates.len() == 1 {
            ""
        } else {
            " of each"
        };
        let fix = if preferences.is_empty() {
            format!("keep one{each} with `exclude path <path>` in the recipe")
        } else {
            format!(
                "no `prefer path` line leaves one path{each}; keep one{each} with `exclude path <path>` or a `prefer path` that covers it alone"
            )
        };
        return Err(error(format!("{}\n  {fix}", duplicates.join("\n"))));
    }
    if let Some(unused) = chose.iter().position(|chose| !chose) {
        let preference = &preferences[unused];
        return Err(error(format!(
            "`{preference}` ({}) chooses for no identity: no identity is read from two or more paths of which it covers some",
            preference.origin
        )));
    }
    let mut index = 0;
    records.retain(|_| {
        index += 1;
        !dropped[index - 1]
    });
    Ok(())
}

fn named(
    first: &SourceRecord,
    kept: &[usize],
    records: &[SourceRecord],
    folders: &FxHashSet<&str>,
) -> String {
    let kind = if folders.contains(first.product.as_str()) {
        "folders"
    } else {
        "files"
    };
    let paths: String = kept
        .iter()
        .filter_map(|&record| records[record].path.as_deref())
        .map(|path| format!("\n    {path}"))
        .collect();
    format!(
        "{} {kind} read as {}:{paths}",
        kept.len(),
        identity(&first.product, first.entities.iter())
    )
}
