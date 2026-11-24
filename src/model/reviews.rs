use std::fmt;
use std::path::PathBuf;

use super::{ArtifactInstance, EntityBinding};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewDef {
    pub name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Pass,
    Fail,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerdictRow {
    pub review: String,
    pub entities: EntityBinding,
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub file: String,
    pub line: usize,
}

impl VerdictRow {
    pub fn origin(&self) -> String {
        format!("{} line {}", self.file, self.line)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRecord {
    pub review: String,
    pub file: PathBuf,
    pub found: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Withheld {
    pub artifact: ArtifactInstance,
    pub review: String,
    pub port: Option<(String, String)>,
    pub outcome: Judged,
    pub withholds: Vec<ArtifactInstance>,
    pub shrinks: Vec<ArtifactInstance>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Judged {
    Rejected {
        reason: Option<String>,
        origin: String,
        line: usize,
    },
    Held,
}

impl fmt::Display for Withheld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let identity = &self.artifact;
        let at = match &self.port {
            Some((operation, port)) => format!(" at `{port}` of `{operation}`"),
            None => String::new(),
        };
        match &self.outcome {
            Judged::Rejected { reason, origin, .. } => {
                write!(
                    f,
                    "review `{}` rejected {identity}{at} ({origin})",
                    self.review
                )?;
                if let Some(reason) = reason {
                    write!(f, ": {reason}")?;
                }
                Ok(())
            }
            Judged::Held => write!(
                f,
                "review `{}` holds back {identity}{at}: no verdict yet",
                self.review
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnmatchedVerdicts {
    pub file: String,
    pub lines: Vec<usize>,
}

impl fmt::Display for UnmatchedVerdicts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.lines.len();
        let (noun, match_) = if count == 1 {
            ("verdict", "matches")
        } else {
            ("verdicts", "match")
        };
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for &line in &self.lines {
            match runs.last_mut() {
                Some((_, end)) if *end + 1 == line => *end = line,
                _ => runs.push((line, line)),
            }
        }
        let runs: Vec<String> = runs
            .iter()
            .map(|&(start, end)| {
                if start == end {
                    start.to_string()
                } else {
                    format!("{start}\u{2013}{end}")
                }
            })
            .collect();
        let lines = if count == 1 { "line" } else { "lines" };
        write!(
            f,
            "{count} {noun} in {} {match_} no artifact ({lines} {})",
            self.file,
            runs.join(", ")
        )
    }
}
