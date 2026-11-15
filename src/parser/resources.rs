//! `resources` lines: what each job of an operation needs to run, as in
//! `resources align: cores=8, memory=16G`.

use crate::model::{Resources, ResourcesDef, MEMORY_UNITS};

use super::declarations::single_colon;
use super::error::ResourceError;
use super::lexical::qualified_identifier;
use super::ParseError;

/// The most memory a job may claim: 2^53 bytes, the largest whole number
/// every JSON reader holds exactly.
const MAX_MEMORY: u64 = 1 << 53;

/// Parse `line`, the text after `resources`: an operation, a `:`, and
/// `cores=N` and `memory=SIZE`, either or both, separated by a comma.
pub(super) fn parse_resources(line: &str, number: usize) -> Result<ResourcesDef, ParseError> {
    let delimiter = single_colon(line).ok_or_else(|| {
        ParseError::of(
            number,
            ResourceError::ExpectedResourcesOperationCoresNMemory,
        )
    })?;
    let (operation, rest) = line.split_at(delimiter);
    let operation = qualified_identifier(operation.trim(), number, "resources operation")?;
    let settings = rest[1..].trim();
    if settings.is_empty() {
        return Err(ParseError::of(
            number,
            ResourceError::ExpectedResourcesOperationCoresNMemory,
        ));
    }
    let mut cores = None;
    let mut memory = None;
    for setting in settings.split(',').map(str::trim) {
        let Some((key, value)) = setting.split_once('=') else {
            let error = if setting.is_empty() {
                ResourceError::EmptySetting {
                    operation: operation.to_owned(),
                }
            } else {
                ResourceError::SettingNotNameValue {
                    setting: setting.to_owned(),
                }
            };
            return Err(ParseError::of(number, error).at_token(setting));
        };
        let (key, value) = (key.trim(), value.trim());
        let fail = |error: ResourceError| Err(ParseError::of(number, error).at_token(setting));
        let (slot, parsed) = match key {
            "cores" => (cores.is_some(), parse_cores(value).map(|n| cores = Some(n))),
            "memory" => (
                memory.is_some(),
                parse_memory(value).map(|n| memory = Some(n)),
            ),
            _ => {
                return fail(ResourceError::NotAResource {
                    key: key.to_owned(),
                })
            }
        };
        if slot {
            return fail(ResourceError::GivenTwice {
                operation: operation.to_owned(),
                key: key.to_owned(),
            });
        }
        if let Err(error) = parsed {
            return fail(error);
        }
    }
    Ok(ResourcesDef {
        operation: operation.to_owned(),
        resources: Resources {
            cores: cores.unwrap_or(1),
            memory,
        },
    })
}

/// The cores a job uses: a whole number, at least 1.
fn parse_cores(value: &str) -> Result<u32, ResourceError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ResourceError::CoresNotWhole {
            value: value.to_owned(),
        });
    }
    match value.parse::<u32>() {
        Ok(0) => Err(ResourceError::CoresZero),
        Ok(cores) => Ok(cores),
        Err(_) => Err(ResourceError::CoresTooMany {
            value: value.to_owned(),
        }),
    }
}

/// The memory a job needs, in bytes: a whole number and a unit, `K`, `M`,
/// `G` or `T`, each 1024 times the one before. Keep in step with
/// `MEMORY_UNITS` in `src/model/definitions.rs`.
fn parse_memory(value: &str) -> Result<u64, ResourceError> {
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    let (number, unit) = value.split_at(digits);
    let shift = match unit.chars().next() {
        Some(first) if unit.len() == 1 => MEMORY_UNITS
            .iter()
            .find(|&&(name, _)| name == first)
            .map(|&(_, shift)| shift),
        _ => None,
    };
    let (Some(shift), false) = (shift, number.is_empty()) else {
        let value = value.to_owned();
        return Err(match unit.chars().next().map(|c| c.to_ascii_uppercase()) {
            _ if number.is_empty() || unit.starts_with('.') => ResourceError::MemoryForm { value },
            None => ResourceError::MemoryNeedsUnit {
                value,
                number: number.to_owned(),
            },
            Some(first) if MEMORY_UNITS.iter().any(|&(name, _)| name == first) => {
                ResourceError::MemoryUnitCase {
                    value,
                    number: number.to_owned(),
                    first: first.to_string(),
                }
            }
            Some(_) => ResourceError::MemoryForm { value },
        });
    };
    let bytes = number
        .parse::<u64>()
        .ok()
        .and_then(|number| number.checked_mul(1 << shift))
        .filter(|&bytes| bytes <= MAX_MEMORY);
    match bytes {
        Some(0) => Err(ResourceError::MemoryZero {
            value: value.to_owned(),
        }),
        Some(bytes) => Ok(bytes),
        None => Err(ResourceError::MemoryTooLarge {
            value: value.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_resources;
    use crate::model::Resources;

    fn parse(line: &str) -> Result<Resources, String> {
        parse_resources(line, 1)
            .map(|def| def.resources)
            .map_err(|error| error.message())
    }

    #[test]
    fn cores_and_memory_are_read_with_their_units() {
        assert_eq!(
            parse("align: cores=8, memory=16G"),
            Ok(Resources {
                cores: 8,
                memory: Some(16 << 30)
            })
        );
        assert_eq!(
            parse("align: memory=512M"),
            Ok(Resources {
                cores: 1,
                memory: Some(512 << 20)
            })
        );
        assert_eq!(
            parse("align: cores = 2").map(|r| r.cores),
            Ok(2),
            "spaces around `=` are allowed"
        );
        let def = parse_resources("L::align: cores=4", 1).expect("a qualified operation");
        assert_eq!(def.operation, "L::align");
    }

    #[test]
    fn bad_values_say_what_to_write() {
        let error = |line: &str| parse(line).expect_err(line);
        assert!(error("a: cores=0").contains("at least one core"));
        assert!(error("a: cores=four").contains("whole number"));
        assert!(error("a: cores=1.5").contains("whole number"));
        assert!(error("a: cores=99999999999").contains("more cores"));
        assert!(error("a: memory=16").contains("`memory=16G`"));
        assert!(error("a: memory=16GB").contains("write `memory=16G`"));
        assert!(error("a: memory=16g").contains("write `memory=16G`"));
        assert!(error("a: memory=1.5G").contains("1536M"));
        assert!(error("a: memory=G").contains("whole number"));
        assert!(error("a: memory=0G").contains("leave `memory` out"));
        assert!(error("a: memory=9000T").contains("8192T"));
        assert!(error("a: threads=4").contains("not a resource"));
        assert!(error("a: cores=2, cores=4").contains("twice"));
        assert!(error("a: cores 4").contains("name=value"));
        assert!(error("a: cores=2,").contains("comma"));
        assert!(error("a:").contains("expected `resources"));
        assert!(error("a cores=2").contains("expected `resources"));
    }
}
