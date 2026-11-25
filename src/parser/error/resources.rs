use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceError {
    ExpectedResources,
    EmptySetting {
        operation: String,
    },
    SettingNotNameValue {
        setting: String,
    },
    NotAResource {
        key: String,
    },
    GivenTwice {
        operation: String,
        key: String,
    },
    CoresNotWhole {
        value: String,
    },
    CoresZero,
    CoresTooMany {
        value: String,
    },
    MemoryForm {
        value: String,
    },
    MemoryNeedsUnit {
        value: String,
        number: String,
    },
    MemoryUnitCase {
        value: String,
        number: String,
        first: String,
    },
    MemoryZero {
        value: String,
    },
    MemoryTooLarge {
        value: String,
    },
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedResources => f.write_str("expected `resources operation: cores=N, memory=SIZE`, as in `resources align: cores=8, memory=16G`"),
            Self::EmptySetting { operation } => write!(f, "a comma in `resources {operation}` separates two settings, as in `cores=8, memory=16G`"),
            Self::SettingNotNameValue { setting } => write!(f, "`{setting}`: write a resource as `name=value`, as in `cores=8` or `memory=16G`"),
            Self::NotAResource { key } => write!(f, "`{key}` is not a resource; a job declares `cores=N` and `memory=SIZE`"),
            Self::GivenTwice { operation, key } => write!(f, "`resources {operation}` gives `{key}` twice"),
            Self::CoresNotWhole { value } => write!(f, "`cores={value}`: give the cores a job uses as a whole number, as in `cores=8`"),
            Self::CoresZero => f.write_str("`cores=0`: a job uses at least one core; leave `cores` out for one"),
            Self::CoresTooMany { value } => write!(f, "`cores={value}` is more cores than any machine has"),
            Self::MemoryForm { value } => write!(f, "`memory={value}`: write a whole number and a unit, as in `memory=16G` or `memory=1536M`; the units are K, M, G and T, each 1024 times the one before"),
            Self::MemoryNeedsUnit { value, number } => write!(f, "`memory={value}`: give it a unit, as in `memory={number}M` or `memory={number}G`; the units are K, M, G and T, each 1024 times the one before"),
            Self::MemoryUnitCase { value, number, first } => write!(f, "`memory={value}`: write `memory={number}{first}`; the units are K, M, G and T, each 1024 times the one before"),
            Self::MemoryZero { value } => write!(f, "`memory={value}`: a job that claims memory claims more than none; leave `memory` out to claim none"),
            Self::MemoryTooLarge { value } => write!(f, "`memory={value}` is more than the 8192T a job may claim"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            ResourceError::ExpectedResources,
            ResourceError::EmptySetting {
                operation: "operation".to_owned(),
            },
            ResourceError::SettingNotNameValue {
                setting: "setting".to_owned(),
            },
            ResourceError::NotAResource {
                key: "key".to_owned(),
            },
            ResourceError::GivenTwice {
                operation: "operation".to_owned(),
                key: "key".to_owned(),
            },
            ResourceError::CoresNotWhole {
                value: "value".to_owned(),
            },
            ResourceError::CoresZero,
            ResourceError::CoresTooMany {
                value: "value".to_owned(),
            },
            ResourceError::MemoryForm {
                value: "value".to_owned(),
            },
            ResourceError::MemoryNeedsUnit {
                value: "value".to_owned(),
                number: "number".to_owned(),
            },
            ResourceError::MemoryUnitCase {
                value: "value".to_owned(),
                number: "number".to_owned(),
                first: "first".to_owned(),
            },
            ResourceError::MemoryZero {
                value: "value".to_owned(),
            },
            ResourceError::MemoryTooLarge {
                value: "value".to_owned(),
            },
        ];
        crate::parser::error::assert_worded(variants);
    }
}
