//! Path components: what makes a path unusable, how a value is encoded in
//! one, and the checks on a discovery rule's folder.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::DirectoryDiscovery;

use super::template::{error, PathError, PathPart, PathPlaceholder};

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

/// `value` as one path component: ASCII letters, digits and `-` as they
/// are, every other byte as `%XX`.
///
/// Keep in step with `is_value_character` in `inputs/discover.rs`, which
/// holds that an encoded value has only these characters and `%`:
/// discovery binds a value without searching when the character after it
/// cannot be one of them, so a character added here must be added there.
pub(crate) fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    push_encoded(&mut encoded, value);
    encoded
}

/// Add `value` to `text` as [`encode_component`] encodes it. Runs of
/// bytes kept as they are are added whole.
pub(crate) fn push_encoded(text: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut plain = 0;
    for (index, byte) in value.bytes().enumerate() {
        if byte.is_ascii_alphanumeric() || byte == b'-' {
            continue;
        }
        // A run kept as it is is ASCII, so it starts and ends between
        // characters; an empty one may not.
        if plain < index {
            text.push_str(&value[plain..index]);
        }
        text.push('%');
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0xF)]));
        plain = index + 1;
    }
    text.push_str(&value[plain..]);
}

pub(crate) fn decode_component(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = encoded.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
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
pub(crate) fn validate_discovery_rule(rule: &DirectoryDiscovery) -> Result<(), PathError> {
    let dimensions: BTreeSet<_> = rule.dimensions.iter().collect();
    if rule.dimensions.is_empty() || dimensions.len() != rule.dimensions.len() {
        return Err(error(format!(
            "discovery `{}` needs distinct dimensions",
            rule.name
        )));
    }
    let mut used = BTreeSet::new();
    let mut sample = String::new();
    for part in rule.template.parts() {
        match part {
            PathPart::Literal(value) => sample.push_str(value),
            PathPart::Group(_) => {
                return Err(error(format!(
                    "discovery `{}` pattern cannot have an optional `[...]` part; every directory it finds has each dimension",
                    rule.name
                )));
            }
            PathPart::Placeholder(PathPlaceholder::Dimension(name))
                if dimensions.contains(name) =>
            {
                used.insert(name);
                sample.push_str(name);
            }
            PathPart::Placeholder(placeholder) => {
                return Err(error(format!(
                    "discovery `{}` uses undeclared or reserved placeholder `{placeholder}`",
                    rule.name
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
            "discovery `{}` pattern omits dimension `{missing}`",
            rule.name
        )));
    }
    if sample.is_empty() || sample.ends_with('/') {
        return Err(error(format!(
            "discovery `{}` must name a directory without a trailing `/`",
            rule.name
        )));
    }
    if let Some(reason) = unusable_path(&sample) {
        return Err(error(format!("discovery `{}` pattern {reason}", rule.name)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decode_component, encode_component, unusable_path};

    #[test]
    fn components_keep_letters_digits_and_dashes_and_encode_the_rest() {
        assert_eq!(encode_component("sub-01"), "sub-01");
        assert_eq!(encode_component("a b/é"), "a%20b%2F%C3%A9");
        assert_eq!(encode_component("éa"), "%C3%A9a");
        assert_eq!(encode_component("__"), "%5F%5F");
        assert_eq!(encode_component(""), "");
        assert_eq!(decode_component("a%20b%2F%C3%A9").as_deref(), Some("a b/é"));
    }

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
