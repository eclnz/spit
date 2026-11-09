//! Path components: what makes a path unusable, and the checks on a
//! discovery rule's folder.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::ContextPattern;

use crate::template::{error, PathError, PathPart, PathPlaceholder};

/// Why `relative` cannot name a file under the root, if it cannot.
pub(crate) fn unusable_path(relative: &str) -> Option<&'static str> {
    if relative.starts_with('/') {
        return Some("must be relative to the dataset root, not start with `/`");
    }
    if relative.ends_with('/') {
        return Some("must name a file, not end with `/`");
    }
    let (mut empty, mut dots) = (false, false);
    for component in relative.as_bytes().split(|&byte| byte == b'/') {
        empty |= component.is_empty();
        dots |= component == b"." || component == b"..";
    }
    if empty {
        Some("must not contain an empty directory name, as in `//`")
    } else if dots {
        Some("must not contain `.` or `..` directories")
    } else {
        None
    }
}

/// A directory of `path` that is itself a file in `paths`, with its owner.
pub(crate) fn enclosing_path<'a, T>(
    paths: &'a BTreeMap<String, T>,
    path: &str,
) -> Option<(&'a str, &'a T)> {
    path.match_indices('/').find_map(|(end, _)| {
        paths
            .get_key_value(&path[..end])
            .map(|(directory, owner)| (directory.as_str(), owner))
    })
}

/// Check that a directory rule names a valid context path.
pub(crate) fn validate_context_pattern(rule: &ContextPattern) -> Result<(), PathError> {
    let dimensions: BTreeSet<_> = rule.dimensions.iter().collect();
    if dimensions.len() != rule.dimensions.len() {
        return Err(error("context path needs distinct dimensions"));
    }
    let mut used = BTreeSet::new();
    let mut sample = String::new();
    for part in rule.template.parts() {
        match part {
            PathPart::Literal(value) => sample.push_str(value),
            PathPart::Group(_) => {
                return Err(error(
                    "context path cannot have an optional `[...]` part; every directory it finds has each dimension",
                ));
            }
            PathPart::Placeholder(PathPlaceholder::Dimension(name, _))
                if dimensions.contains(name) =>
            {
                used.insert(name);
                sample.push_str(name);
            }
            PathPart::Placeholder(placeholder) => {
                return Err(error(format!(
                    "context path uses reserved placeholder `{placeholder}`"
                )));
            }
        }
    }
    if let Some(missing) = rule
        .dimensions
        .iter()
        .find(|dimension| !used.contains(dimension))
    {
        return Err(error(format!(
            "context path pattern omits dimension `{missing}`"
        )));
    }
    if sample.is_empty() || sample.ends_with('/') {
        return Err(error(
            "context path must name a directory without a trailing `/`",
        ));
    }
    if let Some(reason) = unusable_path(&sample) {
        return Err(error(format!("context path pattern {reason}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::unusable_path;

    #[test]
    fn unusable_paths_are_named_by_their_first_problem() {
        assert_eq!(unusable_path("a/b.txt"), None);
        assert!(unusable_path("/a").unwrap().contains("relative"));
        assert!(unusable_path("a/").unwrap().contains("end with"));
        assert!(unusable_path("a//../b").unwrap().contains("empty"));
        assert!(unusable_path("a/../b").unwrap().contains("`..`"));
        assert!(unusable_path("./b").unwrap().contains("`..`"));
    }
}
