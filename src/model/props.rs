//! The properties a `with` line gives jobs. SPIT does not read what a key
//! means: a backend does, such as a runner that reads `cpus` and `mem`.

use std::collections::BTreeMap;

/// One `key=value` of a `with` line. A value of `None` is `key=-`, which
/// takes away what a wider scope gave the key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prop {
    pub key: String,
    pub value: Option<String>,
}

/// The properties of a job, once its scopes are merged: sorted by key, with
/// every key that has a value and none that was taken away.
pub type JobProps = Vec<(String, String)>;

/// Merge `layers`, widest first: a later layer sets or removes the keys it
/// names and leaves the rest as they were.
pub fn merge_props<'a>(layers: impl IntoIterator<Item = &'a [Prop]>) -> JobProps {
    let mut merged: BTreeMap<&str, Option<&str>> = BTreeMap::new();
    for layer in layers {
        for prop in layer {
            merged.insert(&prop.key, prop.value.as_deref());
        }
    }
    merged
        .into_iter()
        .filter_map(|(key, value)| Some((key.to_owned(), value?.to_owned())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{merge_props, Prop};

    fn prop(key: &str, value: Option<&str>) -> Prop {
        Prop {
            key: key.to_owned(),
            value: value.map(str::to_owned),
        }
    }

    #[test]
    fn a_narrower_layer_sets_or_removes_only_the_keys_it_names() {
        let wide = [prop("cpus", Some("1")), prop("mem", Some("2G"))];
        let narrow = [prop("cpus", Some("8")), prop("mem", None)];
        let narrowest = [prop("time", Some("6h"))];
        assert_eq!(
            merge_props([&wide[..], &narrow[..], &narrowest[..]]),
            vec![
                ("cpus".to_owned(), "8".to_owned()),
                ("time".to_owned(), "6h".to_owned())
            ]
        );
    }
}
