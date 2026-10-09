//! The definition of an entity assignment in a path, shared by all shapes.

use std::collections::BTreeMap;

use crate::template::{parse_template, Part};

/// How `{@entities}` writes each dimension. Labels change path text only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntitiesFormat {
    pub prefix: String,
    pub between: String,
    pub suffix: String,
    pub separator: String,
    pub empty: String,
    pub labels: BTreeMap<String, String>,
}

impl Default for EntitiesFormat {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            between: "=".to_owned(),
            suffix: String::new(),
            separator: "__".to_owned(),
            empty: "global".to_owned(),
            labels: BTreeMap::new(),
        }
    }
}

impl EntitiesFormat {
    /// Read one assignment, such as `{key}_{value}`, and its separator.
    pub fn parse(template: &str, separator: &str, empty: &str) -> Result<Self, String> {
        let mut format = Self {
            between: String::new(),
            separator: separator.to_owned(),
            empty: empty.to_owned(),
            ..Self::default()
        };
        let mut phase = 0;
        for part in parse_template(template)? {
            match part {
                Part::Placeholder(name) if phase == 0 && name == "key" => phase = 1,
                Part::Placeholder(name) if phase == 1 && name == "value" => phase = 2,
                Part::Placeholder(_) => return Err(
                    "an entities template needs `{key}` then `{value}`, once each, and no other placeholders".to_owned(),
                ),
                Part::Literal(text) => match phase {
                    0 => format.prefix.push_str(&text),
                    1 => format.between.push_str(&text),
                    _ => format.suffix.push_str(&text),
                },
            }
        }
        if phase != 2 {
            return Err("an entities template needs `{key}` then `{value}`, once each".to_owned());
        }
        format.validate()?;
        Ok(format)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let literals = [&self.prefix, &self.between, &self.suffix, &self.separator];
        if literals.iter().any(|text| {
            text.chars().any(|c| {
                c.is_whitespace()
                    || c.is_control()
                    || matches!(c, '/' | '\\' | '%' | '{' | '}' | '[' | ']')
            })
        }) {
            return Err("entities formatting must stay in one path component; literal text cannot contain whitespace, `/`, `\\`, `%`, braces or brackets".to_owned());
        }
        // Keep in step with `push_encoded` in components.rs: values retain
        // only ASCII letters, digits and `-`; every other byte is %XX.
        let boundary = literals.iter().any(|text| {
            text.bytes()
                .any(|byte| !byte.is_ascii_alphanumeric() && byte != b'-')
        });
        if !boundary {
            return Err("entities formatting is ambiguous: the template or separator needs punctuation other than `-` to separate encoded values".to_owned());
        }
        Ok(())
    }
}
