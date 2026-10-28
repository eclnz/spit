//! `resources` lines: what each job of an operation needs to run, as in
//! `resources align: cores=8, memory=16G`.

use crate::model::{Resources, ResourcesDef, MEMORY_UNITS};

use super::declarations::single_colon;
use super::lexical::qualified_identifier;
use super::ParseError;

const EXPECTED: &str = "expected `resources operation: cores=N, memory=SIZE`, as in `resources align: cores=8, memory=16G`";

/// The most memory a job may claim: 2^53 bytes, the largest whole number
/// every JSON reader holds exactly.
const MAX_MEMORY: u64 = 1 << 53;

/// Parse `line`, the text after `resources`: an operation, a `:`, and
/// `cores=N` and `memory=SIZE`, either or both, separated by a comma.
pub(super) fn parse_resources(line: &str, number: usize) -> Result<ResourcesDef, ParseError> {
    let delimiter = single_colon(line).ok_or_else(|| ParseError::new(number, EXPECTED))?;
    let (operation, rest) = line.split_at(delimiter);
    let operation = qualified_identifier(operation.trim(), number, "resources operation")?;
    let settings = rest[1..].trim();
    if settings.is_empty() {
        return Err(ParseError::new(number, EXPECTED));
    }
    let mut cores = None;
    let mut memory = None;
    for setting in settings.split(',').map(str::trim) {
        let Some((key, value)) = setting.split_once('=') else {
            let message = if setting.is_empty() {
                format!("a comma in `resources {operation}` separates two settings, as in `cores=8, memory=16G`")
            } else {
                format!("`{setting}`: write a resource as `name=value`, as in `cores=8` or `memory=16G`")
            };
            return Err(ParseError::new(number, message).at_token(setting));
        };
        let (key, value) = (key.trim(), value.trim());
        let fail = |message: String| Err(ParseError::new(number, message).at_token(setting));
        let (slot, parsed) = match key {
            "cores" => (cores.is_some(), parse_cores(value).map(|n| cores = Some(n))),
            "memory" => (
                memory.is_some(),
                parse_memory(value).map(|n| memory = Some(n)),
            ),
            _ => {
                return fail(format!(
                    "`{key}` is not a resource; a job declares `cores=N` and `memory=SIZE`"
                ))
            }
        };
        if slot {
            return fail(format!("`resources {operation}` gives `{key}` twice"));
        }
        if let Err(message) = parsed {
            return fail(message);
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
fn parse_cores(value: &str) -> Result<u32, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!(
            "`cores={value}`: give the cores a job uses as a whole number, as in `cores=8`"
        ));
    }
    match value.parse::<u32>() {
        Ok(0) => {
            Err("`cores=0`: a job uses at least one core; leave `cores` out for one".to_owned())
        }
        Ok(cores) => Ok(cores),
        Err(_) => Err(format!(
            "`cores={value}` is more cores than any machine has"
        )),
    }
}

/// The memory a job needs, in bytes: a whole number and a unit, `K`, `M`,
/// `G` or `T`, each 1024 times the one before. Keep in step with
/// `MEMORY_UNITS` in `src/model/definitions.rs`.
fn parse_memory(value: &str) -> Result<u64, String> {
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
        let hint = match unit.chars().next().map(|c| c.to_ascii_uppercase()) {
            _ if number.is_empty() || unit.starts_with('.') => {
                "write a whole number and a unit, as in `memory=16G` or `memory=1536M`".to_owned()
            }
            None => format!("give it a unit, as in `memory={number}M` or `memory={number}G`"),
            Some(first) if MEMORY_UNITS.iter().any(|&(name, _)| name == first) => {
                format!("write `memory={number}{first}`")
            }
            Some(_) => {
                "write a whole number and a unit, as in `memory=16G` or `memory=1536M`".to_owned()
            }
        };
        return Err(format!(
            "`memory={value}`: {hint}; the units are K, M, G and T, each 1024 times the one before"
        ));
    };
    let bytes = number
        .parse::<u64>()
        .ok()
        .and_then(|number| number.checked_mul(1 << shift))
        .filter(|&bytes| bytes <= MAX_MEMORY);
    match bytes {
        Some(0) => Err(format!(
            "`memory={value}`: a job that claims memory claims more than none; leave `memory` out to claim none"
        )),
        Some(bytes) => Ok(bytes),
        None => Err(format!(
            "`memory={value}` is more than the 8192T a job may claim"
        )),
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
