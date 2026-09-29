//! Scanning a root for the contexts and source files an input recipe describes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::Path;

use super::coverage::apply_skips;
use crate::model::{
    ArtifactInstance, EntityBinding, InputRules, Pipeline, ProductDef, SourceInventory,
    SourceRecord,
};
use crate::paths::{
    bind_path, decode_component, encode_component, error, inspect_paths, validate_discovery_rule,
    PathError, PathPart, PathPlaceholder, PathTemplate,
};

/// The source files found under a root, and those skipped.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Discovery {
    pub inventory: SourceInventory,
    /// Each skipped file and why.
    pub skipped: Vec<String>,
}

/// Find source artifacts and contexts under `root`. Directory rules provide
/// contexts and require files for source products whose dimensions they cover;
/// other source products are found by matching their file path rules.
pub fn discover_sources(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
) -> Result<SourceInventory, PathError> {
    discover_source_files(pipeline, rules, root).map(|discovery| discovery.inventory)
}

/// As [`discover_sources`], also listing files that fit a rule but hold no
/// readable value.
///
/// Source files are found by the path rules of `pipeline`, with any in
/// `rules.source_paths` taking precedence.
pub fn discover_source_files(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
) -> Result<Discovery, PathError> {
    let mut pipeline = pipeline.clone();
    pipeline.product_paths.extend(rules.source_paths.clone());
    let pipeline = &pipeline;
    if !root.is_dir() {
        return Err(error(format!(
            "source root is not a directory: `{}`",
            root.display()
        )));
    }
    inspect_paths(pipeline)?;
    for rule in &rules.discoveries {
        validate_discovery_rule(rule)?;
    }
    let outputs: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| &invocation.outputs)
        .collect();
    let mut patterns = Vec::new();
    for product in &pipeline.products {
        if outputs.contains(&product.name) {
            continue;
        }
        let template = pipeline.path_template_for(&product.name).ok_or_else(|| {
            error(format!(
                "no path rule for source `{}`, so its files cannot be discovered",
                product.name
            ))
        })?;
        patterns.push((product, path_pattern(template, product)?));
    }
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut visited = BTreeSet::new();
    walk(root, "", &mut visited, &mut files, &mut directories)?;
    files.sort();
    directories.sort();
    let mut discovery = Discovery::default();
    let directory_patterns: Vec<_> = rules
        .discoveries
        .iter()
        .map(|rule| {
            let pieces = rule
                .template
                .parts()
                .iter()
                .map(|part| match part {
                    PathPart::Literal(value) => Piece::Literal(value.clone()),
                    PathPart::Placeholder(PathPlaceholder::Dimension(name)) => {
                        Piece::Value(name.clone())
                    }
                    _ => unreachable!("validated discovery rule"),
                })
                .collect::<Vec<_>>();
            (rule, pieces)
        })
        .collect();
    let mut contexts = BTreeSet::new();
    let mut rule_contexts = vec![BTreeSet::new(); directory_patterns.len()];
    for directory in &directories {
        for (index, (_, pattern)) in directory_patterns.iter().enumerate() {
            let Some(bound) = match_pattern(pattern, directory) else {
                continue;
            };
            let mut entities = BTreeMap::new();
            let mut valid = true;
            for (dimension, encoded) in bound {
                match readable_value(encoded) {
                    Ok(value) => {
                        entities.insert(dimension, value);
                    }
                    Err(reason) => {
                        discovery.skipped.push(format!(
                            "`{directory}`: `{dimension}` value `{encoded}` {reason}"
                        ));
                        valid = false;
                        break;
                    }
                }
            }
            if valid {
                let binding = EntityBinding(entities);
                contexts.insert(binding.clone());
                rule_contexts[index].insert(binding);
            }
        }
    }
    for ((rule, _), bindings) in directory_patterns.iter().zip(&rule_contexts) {
        if bindings.is_empty() {
            return Err(error(format!(
                "discovery `{}` matched no directories under `{}` with pattern `{}`",
                rule.name,
                root.display(),
                rule.template
            )));
        }
    }
    discovery.inventory.contexts = contexts.into_iter().collect();
    discovery.inventory.discovered = directory_patterns
        .iter()
        .zip(&rule_contexts)
        .map(|((rule, _), bindings)| (rule.name.clone(), bindings.iter().cloned().collect()))
        .collect();
    let skipped_groups = apply_skips(rules, &mut discovery.inventory, true);
    for group in &skipped_groups {
        discovery.skipped.push(group.note());
    }
    let mut expected: BTreeMap<String, BTreeSet<EntityBinding>> = BTreeMap::new();
    for product in &pipeline.products {
        if outputs.contains(&product.name) {
            continue;
        }
        for (rule, _) in &directory_patterns {
            let bindings = &discovery.inventory.discovered[&rule.name];
            if product
                .dimensions
                .iter()
                .all(|dimension| rule.dimensions.contains(dimension))
            {
                for binding in bindings {
                    expected.entry(product.name.clone()).or_default().insert(
                        binding
                            .project(&product.dimensions)
                            .expect("rule binds its dimensions"),
                    );
                }
            }
        }
    }
    'files: for file in &files {
        let mut owner: Option<&ProductDef> = None;
        let mut record = None;
        for (product, pattern) in &patterns {
            let Some(bound) = match_pattern(pattern, file) else {
                continue;
            };
            if let Some(other) = owner {
                return Err(error(format!(
                    "file `{file}` matches the path rules of both `{}` and `{}`",
                    other.name, product.name
                )));
            }
            owner = Some(product);
            let mut entities = BTreeMap::new();
            for (dimension, encoded) in bound {
                match readable_value(encoded) {
                    Ok(value) => {
                        entities.insert(dimension, value);
                    }
                    Err(reason) => {
                        discovery.skipped.push(format!(
                            "`{file}`: `{dimension}` value `{encoded}` {reason}"
                        ));
                        continue 'files;
                    }
                }
            }
            let binding = EntityBinding(entities);
            if skipped_groups.iter().any(|group| group.matches(&binding)) {
                continue 'files;
            }
            if let Some(bindings) = expected.get(&product.name) {
                if !bindings.contains(&binding) {
                    return Err(error(format!(
                        "source file `{file}` for `{}` lies outside the discovered contexts",
                        product.name
                    )));
                }
            }
            record = Some(SourceRecord::new(&product.name, binding).at(file.clone()));
        }
        discovery.inventory.artifacts.extend(record);
    }
    discovery.inventory.artifacts = discovery
        .inventory
        .artifacts
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let additional_skips = apply_skips(rules, &mut discovery.inventory, false);
    for group in &additional_skips {
        discovery.skipped.push(group.note());
    }
    for product in &pipeline.products {
        let Some(bindings) = expected.get(&product.name) else {
            continue;
        };
        for binding in bindings {
            if additional_skips.iter().any(|group| group.matches(binding)) {
                continue;
            }
            let artifact = ArtifactInstance::new(
                &product.name,
                product.artifact_type.clone(),
                binding.clone(),
            );
            let relative = bind_path(pipeline, &product.dimensions, &artifact, || {
                format!("source `{artifact}`")
            })?;
            let full = root.join(&relative);
            if !full.is_file() {
                return Err(error(format!(
                    "missing source file for `{artifact}` at discovered context: `{}`",
                    full.display()
                )));
            }
        }
    }
    let rank: BTreeMap<_, _> = pipeline
        .products
        .iter()
        .enumerate()
        .map(|(index, product)| (product.name.as_str(), (index, &product.dimensions)))
        .collect();
    discovery.inventory.artifacts.sort_by(|left, right| {
        let (left_rank, dimensions) = rank[left.product.as_str()];
        left_rank
            .cmp(&rank[right.product.as_str()].0)
            .then_with(|| left.entities.cmp_in(&right.entities, dimensions))
    });
    Ok(discovery)
}

/// Decode a path component. It must be written exactly as SPIT would write
/// it, so the record's path is the file found.
fn readable_value(encoded: &str) -> Result<String, &'static str> {
    let value = decode_component(encoded).ok_or("is not valid `%XX` text")?;
    if encode_component(&value) != encoded {
        return Err("is not how SPIT writes a value, so a path made from it would differ");
    }
    if value
        .chars()
        .any(|character| character.is_whitespace() || ",[]=#".contains(character))
    {
        return Err("holds a space or one of `,[]=#`, which an inventory cannot");
    }
    Ok(value)
}

enum Piece {
    Literal(String),
    /// One path component value, as `bind_path` encodes it.
    Value(String),
}

fn path_pattern(template: &PathTemplate, product: &ProductDef) -> Result<Vec<Piece>, PathError> {
    let mut pieces = Vec::new();
    for part in template.parts() {
        match part {
            PathPart::Literal(value) => pieces.push(Piece::Literal(value.clone())),
            PathPart::Placeholder(PathPlaceholder::Product) => {
                pieces.push(Piece::Literal(product.name.replace("::", ".")));
            }
            PathPart::Placeholder(PathPlaceholder::Entities) => {
                if product.dimensions.is_empty() {
                    pieces.push(Piece::Literal("global".to_owned()));
                }
                for (index, dimension) in product.dimensions.iter().enumerate() {
                    let separator = if index == 0 { "" } else { "__" };
                    pieces.push(Piece::Literal(format!(
                        "{separator}{}=",
                        encode_component(dimension)
                    )));
                    pieces.push(Piece::Value(dimension.clone()));
                }
            }
            // A source is made in no stage, so its rule never binds
            // `{stage}`; `inspect_paths` rejects such a rule first.
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                return Err(error(format!(
                    "path rule for source `{}` uses `{}`, but a source is not made in a stage",
                    product.name,
                    PathPlaceholder::Stage
                )))
            }
            PathPart::Placeholder(PathPlaceholder::Dimension(dimension)) => {
                pieces.push(Piece::Value(dimension.clone()));
            }
        }
    }
    Ok(pieces)
}

/// Match `text` against `pieces`, binding each dimension to its encoded
/// value; a dimension used twice must have the same value both times.
fn match_pattern<'a>(pieces: &[Piece], text: &'a str) -> Option<BTreeMap<String, &'a str>> {
    let mut bound = BTreeMap::new();
    let mut failed = BTreeSet::new();
    match_from(pieces, 0, text, 0, &mut bound, &mut failed).then(|| {
        bound
            .into_iter()
            .map(|(dimension, value)| (dimension, &text[value]))
            .collect()
    })
}

/// A position that failed to match: the piece, the offset in the text, and
/// the values bound for dimensions that later pieces repeat.
type Attempt = (usize, usize, Vec<(usize, usize)>);

/// Match `pieces[index..]` against `text[offset..]`, binding each dimension
/// to the byte range of its value in `text`. Failed positions are
/// remembered, which keeps ambiguous splits from taking exponential time.
fn match_from(
    pieces: &[Piece],
    index: usize,
    text: &str,
    offset: usize,
    bound: &mut BTreeMap<String, Range<usize>>,
    failed: &mut BTreeSet<Attempt>,
) -> bool {
    let rest = &text[offset..];
    let Some(piece) = pieces.get(index) else {
        return rest.is_empty();
    };
    let later: Vec<_> = bound
        .iter()
        .filter(|(dimension, _)| {
            pieces[index..]
                .iter()
                .any(|piece| matches!(piece, Piece::Value(name) if name == *dimension))
        })
        .map(|(_, value)| (value.start, value.end))
        .collect();
    let attempt = (index, offset, later);
    if failed.contains(&attempt) {
        return false;
    }
    let matched = match piece {
        Piece::Literal(literal) => {
            rest.starts_with(literal.as_str())
                && match_from(
                    pieces,
                    index + 1,
                    text,
                    offset + literal.len(),
                    bound,
                    failed,
                )
        }
        Piece::Value(dimension) => match bound.get(dimension).map(|value| &text[value.clone()]) {
            Some(value) => {
                rest.starts_with(value)
                    && match_from(pieces, index + 1, text, offset + value.len(), bound, failed)
            }
            None => {
                let longest = rest
                    .find(|character: char| {
                        !(character.is_ascii_alphanumeric() || character == '-' || character == '%')
                    })
                    .unwrap_or(rest.len());
                let found = (1..=longest).any(|end| {
                    bound.insert(dimension.clone(), offset..offset + end);
                    match_from(pieces, index + 1, text, offset + end, bound, failed)
                });
                if !found {
                    bound.remove(dimension);
                }
                found
            }
        },
    };
    if !matched {
        failed.insert(attempt);
    }
    matched
}

/// Collect every regular file under `directory`, as `/`-separated paths
/// relative to the root, following links. `visited` stops link cycles.
/// Names that are not UTF-8 cannot match a rule.
fn walk(
    directory: &Path,
    prefix: &str,
    visited: &mut BTreeSet<std::path::PathBuf>,
    files: &mut Vec<String>,
    directories: &mut Vec<String>,
) -> Result<(), PathError> {
    let unreadable =
        |reason: std::io::Error| error(format!("cannot read `{}`: {reason}", directory.display()));
    if !visited.insert(fs::canonicalize(directory).map_err(unreadable)?) {
        return Ok(());
    }
    let entries = fs::read_dir(directory).map_err(unreadable)?;
    for entry in entries {
        let entry = entry.map_err(unreadable)?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let relative = format!("{prefix}{name}");
        let path = entry.path();
        if path.is_dir() {
            directories.push(relative.clone());
            walk(&path, &format!("{relative}/"), visited, files, directories)?;
        } else if path.is_file() {
            files.push(relative);
        }
    }
    Ok(())
}

/// Give each record without a path the one its source's rule gives it, so
/// that resolving jobs needs no rule for a source. `rules.source_paths` take
/// precedence over the pipeline's. A source with no rule keeps no path.
pub(crate) fn locate_sources(
    pipeline: &Pipeline,
    rules: &InputRules,
    inventory: &mut SourceInventory,
) -> Result<(), PathError> {
    let mut pipeline = pipeline.clone();
    pipeline.product_paths.extend(rules.source_paths.clone());
    for record in &mut inventory.artifacts {
        if record.path.is_some() || pipeline.path_template_for(&record.product).is_none() {
            continue;
        }
        let Some(product) = pipeline
            .products
            .iter()
            .find(|product| product.name == record.product)
        else {
            continue;
        };
        let artifact = ArtifactInstance::new(
            &product.name,
            product.artifact_type.clone(),
            record.entities.clone(),
        );
        let path = bind_path(&pipeline, &product.dimensions, &artifact, || {
            format!("source `{artifact}`")
        })?;
        record.path = Some(path);
    }
    Ok(())
}
