//! The bound DAG: what step 3 hands a backend. Every job carries its
//! artifacts' paths and its commands as argument lists, so a backend such as
//! Bash needs nothing else; it never sees the pipeline, a path rule or a
//! command template. Written as a `.spitdag`, a JSON document.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::model::stage_within;
use crate::types::TypeExpr;

/// The schema version a `.spitdag` is written with.
pub const SPITDAG_VERSION: u64 = 2;

/// A resolved DAG with its paths bound and its commands expanded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundDag {
    pub jobs: Vec<BoundJob>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundJob {
    pub id: usize,
    pub operation: String,
    /// The stage of the step that made this job, as `outer/inner`.
    pub stage: Option<String>,
    /// Each input port and its artifacts, in port order.
    pub inputs: Vec<(String, Vec<BoundArtifact>)>,
    /// Each output port and its artifact, in port order.
    pub outputs: Vec<(String, BoundArtifact)>,
    pub depends_on: Vec<usize>,
    /// The command that makes the outputs; `None` when the operation has none.
    pub command: Option<Vec<Argument>>,
    /// Commands that check the inputs before the job runs.
    pub verify: Vec<Vec<Argument>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundArtifact {
    pub product: String,
    /// Each dimension and value, in the product's declared order.
    pub entities: Vec<(String, String)>,
    pub artifact_type: TypeExpr,
    /// The file, relative to the dataset root.
    pub path: String,
}

/// One command-line argument: literal text and artifact paths, joined.
pub type Argument = Vec<ArgPart>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArgPart {
    Text(String),
    /// A file relative to the dataset root.
    Path(String),
}

impl BoundArtifact {
    /// Its product and bindings, as `image[sub=1,ses=2]`.
    pub fn identity(&self) -> String {
        let bindings: Vec<_> = self
            .entities
            .iter()
            .map(|(dimension, value)| format!("{dimension}={value}"))
            .collect();
        format!("{}[{}]", self.product, bindings.join(","))
    }
}

impl BoundJob {
    /// Every input artifact, in port order.
    pub fn input_artifacts(&self) -> impl Iterator<Item = &BoundArtifact> {
        self.inputs.iter().flat_map(|(_, artifacts)| artifacts)
    }
}

impl BoundDag {
    /// The inputs no job here makes, each once, by path: sources, and the
    /// outputs of stages left out.
    pub fn external_inputs(&self) -> Vec<&BoundArtifact> {
        let produced: BTreeSet<_> = self
            .jobs
            .iter()
            .flat_map(|job| &job.outputs)
            .map(|(_, artifact)| artifact.path.as_str())
            .collect();
        let mut external = BTreeMap::new();
        for artifact in self.jobs.iter().flat_map(BoundJob::input_artifacts) {
            if !produced.contains(artifact.path.as_str()) {
                external.entry(artifact.path.as_str()).or_insert(artifact);
            }
        }
        external.into_values().collect()
    }

    /// Only the jobs of `stage` and the stages nested in it. What they read
    /// from other stages becomes an external input that must already exist.
    pub fn only_stage(&self, stage: &str) -> Result<Self, String> {
        let jobs: Vec<_> = self
            .jobs
            .iter()
            .filter(|job| {
                job.stage
                    .as_deref()
                    .is_some_and(|name| stage_within(name, stage))
            })
            .cloned()
            .collect();
        if jobs.is_empty() {
            let stages: BTreeSet<_> = self
                .jobs
                .iter()
                .filter_map(|job| job.stage.as_deref())
                .map(|name| format!("`{name}`"))
                .collect();
            return Err(if stages.is_empty() {
                format!("no jobs in stage `{stage}`; these jobs are in no stage")
            } else {
                let stages: Vec<_> = stages.into_iter().collect();
                format!(
                    "no jobs in stage `{stage}`; stages with jobs: {}",
                    stages.join(", ")
                )
            });
        }
        let kept: BTreeSet<_> = jobs.iter().map(|job| job.id).collect();
        Ok(Self {
            jobs: jobs
                .into_iter()
                .map(|mut job| {
                    job.depends_on.retain(|id| kept.contains(id));
                    job
                })
                .collect(),
        })
    }

    /// The `.spitdag` document.
    pub fn to_json(&self) -> String {
        let mut output = format!("{{\"version\":{SPITDAG_VERSION},\"external_inputs\":[");
        for (index, artifact) in self.external_inputs().into_iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            write_artifact(&mut output, artifact);
        }
        output.push_str("],\"jobs\":[");
        for (index, job) in self.jobs.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            write_job(&mut output, job);
        }
        output.push_str("]}\n");
        output
    }

    /// Read a `.spitdag` document.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let document = json::parse(text)?;
        let version = document.field("version")?.number()?;
        if version != SPITDAG_VERSION {
            return Err(format!(
                "this .spitdag has schema version {version}; this SPIT reads version {SPITDAG_VERSION}"
            ));
        }
        let jobs = document
            .field("jobs")?
            .array()?
            .iter()
            .map(read_job)
            .collect::<Result<_, _>>()?;
        Ok(Self { jobs })
    }
}

/// The jobs as text, each artifact with its path when `paths` is set.
pub fn render_bound_dag(dag: &BoundDag, paths: bool) -> String {
    let mut output = String::new();
    let write_artifact = |output: &mut String, port: Option<&str>, artifact: &BoundArtifact| {
        let mut rendered = artifact.identity();
        if artifact.artifact_type != TypeExpr::Unknown {
            write!(rendered, " : {}", artifact.artifact_type).unwrap();
        }
        match port {
            Some(port) => writeln!(output, "    {port}: {rendered}").unwrap(),
            None => writeln!(output, "    {rendered}").unwrap(),
        }
        if paths {
            writeln!(output, "      path: {}", artifact.path).unwrap();
        }
    };
    for (index, job) in dag.jobs.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        writeln!(output, "Job {}", job.id).unwrap();
        if let Some(stage) = &job.stage {
            writeln!(output, "  stage: {stage}").unwrap();
        }
        writeln!(output, "  operation: {}", job.operation).unwrap();
        writeln!(output, "  inputs:").unwrap();
        for (port, artifacts) in &job.inputs {
            for artifact in artifacts {
                write_artifact(&mut output, Some(port), artifact);
            }
        }
        if let [(_, artifact)] = job.outputs.as_slice() {
            writeln!(output, "  output:").unwrap();
            write_artifact(&mut output, None, artifact);
        } else {
            writeln!(output, "  outputs:").unwrap();
            for (port, artifact) in &job.outputs {
                write_artifact(&mut output, Some(port), artifact);
            }
        }
        if !job.depends_on.is_empty() {
            let dependencies: Vec<_> = job.depends_on.iter().map(ToString::to_string).collect();
            writeln!(output, "  depends_on: {}", dependencies.join(", ")).unwrap();
        }
    }
    output
}

fn write_job(output: &mut String, job: &BoundJob) {
    write!(output, "{{\"id\":{},\"operation\":", job.id).unwrap();
    json::write_string(output, &job.operation);
    output.push_str(",\"stage\":[");
    if let Some(stage) = &job.stage {
        for (index, component) in stage.split('/').enumerate() {
            if index > 0 {
                output.push(',');
            }
            json::write_string(output, component);
        }
    }
    output.push_str("],\"inputs\":{");
    for (index, (port, artifacts)) in job.inputs.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        json::write_string(output, port);
        output.push_str(":[");
        for (index, artifact) in artifacts.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            write_artifact(output, artifact);
        }
        output.push(']');
    }
    output.push_str("},\"outputs\":{");
    for (index, (port, artifact)) in job.outputs.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        json::write_string(output, port);
        output.push(':');
        write_artifact(output, artifact);
    }
    output.push_str("},\"depends_on\":[");
    let dependencies: Vec<_> = job.depends_on.iter().map(ToString::to_string).collect();
    output.push_str(&dependencies.join(","));
    output.push_str("],\"command\":");
    match &job.command {
        Some(command) => write_command(output, command),
        None => output.push_str("null"),
    }
    output.push_str(",\"verify\":[");
    for (index, command) in job.verify.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        write_command(output, command);
    }
    output.push_str("]}");
}

/// An argument is an array of parts: a string for text, `{"path": ...}` for
/// a file.
fn write_command(output: &mut String, command: &[Argument]) {
    output.push('[');
    for (index, argument) in command.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push('[');
        for (index, part) in argument.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            match part {
                ArgPart::Text(text) => json::write_string(output, text),
                ArgPart::Path(path) => {
                    output.push_str("{\"path\":");
                    json::write_string(output, path);
                    output.push('}');
                }
            }
        }
        output.push(']');
    }
    output.push(']');
}

fn write_artifact(output: &mut String, artifact: &BoundArtifact) {
    output.push_str("{\"product\":");
    json::write_string(output, &artifact.product);
    output.push_str(",\"entities\":{");
    for (index, (dimension, value)) in artifact.entities.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        json::write_string(output, dimension);
        output.push(':');
        json::write_string(output, value);
    }
    output.push_str("},\"type\":");
    write_type(output, &artifact.artifact_type);
    output.push_str(",\"path\":");
    json::write_string(output, &artifact.path);
    output.push('}');
}

fn write_type(output: &mut String, artifact_type: &TypeExpr) {
    match artifact_type {
        TypeExpr::Unknown => output.push_str("null"),
        TypeExpr::Variable(name) => {
            output.push_str("{\"variable\":");
            json::write_string(output, name);
            output.push('}');
        }
        TypeExpr::Named(name) => {
            output.push_str("{\"name\":");
            json::write_string(output, name);
            output.push_str(",\"args\":[]}");
        }
        TypeExpr::Applied { constructor, args } => {
            output.push_str("{\"name\":");
            json::write_string(output, constructor);
            output.push_str(",\"args\":[");
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_type(output, arg);
            }
            output.push_str("]}");
        }
    }
}

fn read_job(value: &json::Value) -> Result<BoundJob, String> {
    let id = usize::try_from(value.field("id")?.number()?).map_err(|_| "job id is too large")?;
    let stage: Vec<_> = value
        .field("stage")?
        .array()?
        .iter()
        .map(|component| component.string().map(str::to_owned))
        .collect::<Result<_, _>>()?;
    let inputs = value
        .field("inputs")?
        .object()?
        .iter()
        .map(|(port, artifacts)| {
            let artifacts = artifacts
                .array()?
                .iter()
                .map(read_artifact)
                .collect::<Result<_, _>>()?;
            Ok((port.clone(), artifacts))
        })
        .collect::<Result<_, String>>()?;
    let outputs = value
        .field("outputs")?
        .object()?
        .iter()
        .map(|(port, artifact)| Ok((port.clone(), read_artifact(artifact)?)))
        .collect::<Result<_, String>>()?;
    let depends_on = value
        .field("depends_on")?
        .array()?
        .iter()
        .map(|id| {
            id.number()
                .and_then(|id| usize::try_from(id).map_err(|_| "job id is too large".to_owned()))
        })
        .collect::<Result<_, _>>()?;
    let command = match value.field("command")? {
        json::Value::Null => None,
        command => Some(read_command(command)?),
    };
    let verify = value
        .field("verify")?
        .array()?
        .iter()
        .map(read_command)
        .collect::<Result<_, _>>()?;
    Ok(BoundJob {
        id,
        operation: value.field("operation")?.string()?.to_owned(),
        stage: (!stage.is_empty()).then(|| stage.join("/")),
        inputs,
        outputs,
        depends_on,
        command,
        verify,
    })
}

fn read_command(value: &json::Value) -> Result<Vec<Argument>, String> {
    value
        .array()?
        .iter()
        .map(|argument| {
            argument
                .array()?
                .iter()
                .map(|part| match part {
                    json::Value::String(text) => Ok(ArgPart::Text(text.clone())),
                    part => Ok(ArgPart::Path(part.field("path")?.string()?.to_owned())),
                })
                .collect()
        })
        .collect()
}

fn read_artifact(value: &json::Value) -> Result<BoundArtifact, String> {
    let entities = value
        .field("entities")?
        .object()?
        .iter()
        .map(|(dimension, value)| Ok((dimension.clone(), value.string()?.to_owned())))
        .collect::<Result<_, String>>()?;
    Ok(BoundArtifact {
        product: value.field("product")?.string()?.to_owned(),
        entities,
        artifact_type: read_type(value.field("type")?)?,
        path: value.field("path")?.string()?.to_owned(),
    })
}

fn read_type(value: &json::Value) -> Result<TypeExpr, String> {
    if let json::Value::Null = value {
        return Ok(TypeExpr::Unknown);
    }
    if let Ok(variable) = value.field("variable") {
        return Ok(TypeExpr::Variable(variable.string()?.to_owned()));
    }
    let name = value.field("name")?.string()?.to_owned();
    let args: Vec<_> = value
        .field("args")?
        .array()?
        .iter()
        .map(read_type)
        .collect::<Result<_, _>>()?;
    Ok(if args.is_empty() {
        TypeExpr::Named(name)
    } else {
        TypeExpr::Applied {
            constructor: name,
            args,
        }
    })
}

/// Just enough JSON for a `.spitdag`: objects keep their key order.
mod json {
    use std::fmt::Write;

    #[derive(Debug)]
    pub(super) enum Value {
        Null,
        Bool,
        Number(f64),
        String(String),
        Array(Vec<Value>),
        Object(Vec<(String, Value)>),
    }

    impl Value {
        pub(super) fn field(&self, name: &str) -> Result<&Value, String> {
            self.object()?
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
                .ok_or_else(|| format!("expected a `{name}` field"))
        }

        pub(super) fn object(&self) -> Result<&[(String, Value)], String> {
            match self {
                Self::Object(fields) => Ok(fields),
                _ => Err("expected a JSON object".into()),
            }
        }

        pub(super) fn array(&self) -> Result<&[Value], String> {
            match self {
                Self::Array(items) => Ok(items),
                _ => Err("expected a JSON array".into()),
            }
        }

        pub(super) fn string(&self) -> Result<&str, String> {
            match self {
                Self::String(text) => Ok(text),
                _ => Err("expected a JSON string".into()),
            }
        }

        /// A whole number, as every number in a `.spitdag` is.
        pub(super) fn number(&self) -> Result<u64, String> {
            match self {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                Self::Number(number) if number.fract() == 0.0 && *number >= 0.0 => {
                    Ok(*number as u64)
                }
                _ => Err("expected a whole number".into()),
            }
        }
    }

    pub(super) fn parse(text: &str) -> Result<Value, String> {
        let mut parser = Parser {
            chars: text.char_indices().peekable(),
            text,
        };
        let value = parser.value()?;
        parser.space();
        match parser.chars.next() {
            None => Ok(value),
            Some((at, _)) => Err(format!(
                "unexpected text at byte {at} after the JSON document"
            )),
        }
    }

    struct Parser<'a> {
        chars: std::iter::Peekable<std::str::CharIndices<'a>>,
        text: &'a str,
    }

    impl Parser<'_> {
        fn space(&mut self) {
            while self
                .chars
                .peek()
                .is_some_and(|(_, character)| character.is_whitespace())
            {
                self.chars.next();
            }
        }

        fn expect(&mut self, wanted: char) -> Result<(), String> {
            self.space();
            match self.chars.next() {
                Some((_, character)) if character == wanted => Ok(()),
                Some((at, character)) => Err(format!(
                    "expected `{wanted}` at byte {at}, found `{character}`"
                )),
                None => Err(format!("expected `{wanted}`, found the end of the text")),
            }
        }

        fn word(&mut self, word: &str, value: Value) -> Result<Value, String> {
            for wanted in word.chars() {
                match self.chars.next() {
                    Some((_, character)) if character == wanted => {}
                    _ => return Err(format!("expected `{word}`")),
                }
            }
            Ok(value)
        }

        fn value(&mut self) -> Result<Value, String> {
            self.space();
            match self.chars.peek().copied() {
                Some((_, '{')) => self.object(),
                Some((_, '[')) => self.array(),
                Some((_, '"')) => self.string().map(Value::String),
                Some((_, 'n')) => self.word("null", Value::Null),
                Some((_, 't')) => self.word("true", Value::Bool),
                Some((_, 'f')) => self.word("false", Value::Bool),
                Some((start, character)) if character == '-' || character.is_ascii_digit() => {
                    let mut end = start;
                    while let Some(&(at, character)) = self.chars.peek() {
                        if !(character.is_ascii_digit() || "+-.eE".contains(character)) {
                            break;
                        }
                        end = at + character.len_utf8();
                        self.chars.next();
                    }
                    self.text[start..end]
                        .parse()
                        .map(Value::Number)
                        .map_err(|_| format!("bad number at byte {start}"))
                }
                Some((at, character)) => Err(format!("unexpected `{character}` at byte {at}")),
                None => Err("unexpected end of the text".into()),
            }
        }

        fn object(&mut self) -> Result<Value, String> {
            self.expect('{')?;
            let mut fields = Vec::new();
            self.space();
            if self
                .chars
                .peek()
                .is_some_and(|(_, character)| *character == '}')
            {
                self.chars.next();
                return Ok(Value::Object(fields));
            }
            loop {
                self.space();
                let key = self.string()?;
                self.expect(':')?;
                fields.push((key, self.value()?));
                self.space();
                match self.chars.next() {
                    Some((_, ',')) => {}
                    Some((_, '}')) => return Ok(Value::Object(fields)),
                    _ => return Err("expected `,` or `}` in an object".into()),
                }
            }
        }

        fn array(&mut self) -> Result<Value, String> {
            self.expect('[')?;
            let mut items = Vec::new();
            self.space();
            if self
                .chars
                .peek()
                .is_some_and(|(_, character)| *character == ']')
            {
                self.chars.next();
                return Ok(Value::Array(items));
            }
            loop {
                items.push(self.value()?);
                self.space();
                match self.chars.next() {
                    Some((_, ',')) => {}
                    Some((_, ']')) => return Ok(Value::Array(items)),
                    _ => return Err("expected `,` or `]` in an array".into()),
                }
            }
        }

        fn string(&mut self) -> Result<String, String> {
            self.expect('"')?;
            let mut text = String::new();
            loop {
                match self.chars.next() {
                    Some((_, '"')) => return Ok(text),
                    Some((_, '\\')) => match self.chars.next() {
                        Some((_, '"')) => text.push('"'),
                        Some((_, '\\')) => text.push('\\'),
                        Some((_, '/')) => text.push('/'),
                        Some((_, 'n')) => text.push('\n'),
                        Some((_, 'r')) => text.push('\r'),
                        Some((_, 't')) => text.push('\t'),
                        Some((_, 'b')) => text.push('\u{8}'),
                        Some((_, 'f')) => text.push('\u{c}'),
                        Some((_, 'u')) => text.push(self.unicode()?),
                        _ => return Err("bad escape in a JSON string".into()),
                    },
                    Some((_, character)) => text.push(character),
                    None => return Err("unterminated JSON string".into()),
                }
            }
        }

        /// The character after `\u`, joining a surrogate pair.
        fn unicode(&mut self) -> Result<char, String> {
            let first = self.hex()?;
            if !(0xD800..0xDC00).contains(&first) {
                return char::from_u32(first).ok_or_else(|| "bad `\\u` escape".into());
            }
            self.word("\\u", Value::Null)?;
            let second = self.hex()?;
            char::from_u32(0x10000 + ((first - 0xD800) << 10) + (second.wrapping_sub(0xDC00)))
                .ok_or_else(|| "bad surrogate pair".into())
        }

        fn hex(&mut self) -> Result<u32, String> {
            let mut value = 0;
            for _ in 0..4 {
                let digit = self
                    .chars
                    .next()
                    .and_then(|(_, character)| character.to_digit(16))
                    .ok_or("bad `\\u` escape")?;
                value = value * 16 + digit;
            }
            Ok(value)
        }
    }

    pub(super) fn write_string(output: &mut String, value: &str) {
        output.push('"');
        for character in value.chars() {
            match character {
                '"' => output.push_str("\\\""),
                '\\' => output.push_str("\\\\"),
                '\n' => output.push_str("\\n"),
                '\r' => output.push_str("\\r"),
                '\t' => output.push_str("\\t"),
                c if c < ' ' => write!(output, "\\u{:04x}", u32::from(c)).unwrap(),
                c => output.push(c),
            }
        }
        output.push('"');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(product: &str, path: &str) -> BoundArtifact {
        BoundArtifact {
            product: product.into(),
            entities: vec![
                ("sub".into(), "1".into()),
                ("run".into(), "a\"\\\n\u{1}é".into()),
            ],
            artifact_type: TypeExpr::applied(
                "MRI",
                vec![
                    TypeExpr::applied("Pair", vec![TypeExpr::named("T1w"), TypeExpr::Unknown]),
                    TypeExpr::named("Diffusion"),
                ],
            ),
            path: path.into(),
        }
    }

    #[test]
    fn a_spitdag_reads_back_as_written() {
        let dag = BoundDag {
            jobs: vec![
                BoundJob {
                    id: 1,
                    operation: "clean".into(),
                    stage: Some("prep/denoise".into()),
                    inputs: vec![("raw".into(), vec![artifact("raw", "in/1.txt")])],
                    outputs: vec![("output".into(), artifact("clean", "out/1.txt"))],
                    depends_on: vec![],
                    command: Some(vec![
                        vec![ArgPart::Text("tool".into())],
                        vec![
                            ArgPart::Text("--in=".into()),
                            ArgPart::Path("in/1.txt".into()),
                        ],
                    ]),
                    verify: vec![vec![vec![ArgPart::Path("in/1.txt".into())]]],
                },
                BoundJob {
                    id: 2,
                    operation: "mean".into(),
                    stage: None,
                    inputs: vec![("frames".into(), vec![artifact("clean", "out/1.txt")])],
                    outputs: vec![("output".into(), artifact("mean", "out/mean.txt"))],
                    depends_on: vec![1],
                    command: None,
                    verify: vec![],
                },
            ],
        };
        let text = dag.to_json();
        assert!(text.starts_with("{\"version\":2,\"external_inputs\":[{\"product\":\"raw\""));
        assert_eq!(BoundDag::from_json(&text).unwrap(), dag);
    }

    #[test]
    fn another_schema_version_is_refused() {
        let error = BoundDag::from_json("{\"version\":1,\"jobs\":[]}").unwrap_err();
        assert!(error.contains("schema version 1"), "{error}");
    }
}
