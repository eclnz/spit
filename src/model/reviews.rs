//! Reviews: the verdicts a person records for the artifacts a pipeline
//! attaches a `review` to, and what a plan withheld for them.

use std::fmt;
use std::path::PathBuf;

use super::{ArtifactInstance, EntityBinding};

/// A `review name` declaration. A review has no parameters and no command:
/// a person judges the artifacts it is attached to, and records each
/// verdict in the file a recipe's `review name from file.csv` line names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewDef {
    pub name: String,
}

/// What a person decided about an artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// The artifact is used as usual.
    Pass,
    /// The artifact is rejected: no reader the review covers uses it.
    Fail,
}

impl Verdict {
    /// The word a review file and a `.spitout` write for it.
    pub fn word(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

/// One row of a review's file: the artifacts it judges and its verdict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerdictRow {
    /// The review the row belongs to.
    pub review: String,
    /// The dimensions the row gives a value for. A dimension it leaves empty
    /// is absent, so the row covers every value of it.
    pub entities: EntityBinding,
    pub verdict: Verdict,
    pub reason: Option<String>,
    /// The file the row is read from, as the recipe writes it.
    pub file: String,
    /// The row's line in `file`, from 1.
    pub line: usize,
}

impl VerdictRow {
    /// Where the row is written, as `qc/looked_at.csv line 4`.
    pub fn origin(&self) -> String {
        format!("{} line {}", self.file, self.line)
    }
}

/// A review's file as the plan read it: where it is, and whether it existed.
/// A review the recipe names no file for has none.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRecord {
    pub review: String,
    /// The file: under the recipe's folder, or under a `.spitout`'s folder
    /// unless absolute, as the inventory's `root` is.
    pub file: PathBuf,
    /// Whether the file existed. One that does not has no verdicts.
    pub found: bool,
}

/// An artifact a review keeps from the readers it covers, and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Withheld {
    pub artifact: ArtifactInstance,
    pub review: String,
    /// Where the review is attached, when it is on an input port: the
    /// operation and the port. `None` for a source or an output, which
    /// every reader of the product is kept from.
    pub port: Option<(String, String)>,
    pub outcome: Judged,
    /// The outputs not planned because of it, directly or through other
    /// outputs withheld in turn, in the order their jobs are met.
    pub withholds: Vec<ArtifactInstance>,
    /// The outputs of jobs planned without it, as a member of a `many`
    /// input, in the order their jobs are met.
    pub shrinks: Vec<ArtifactInstance>,
}

/// How a review judged an artifact it withholds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Judged {
    /// A row gave the verdict `fail`.
    Rejected {
        reason: Option<String>,
        /// The row, as [`VerdictRow::origin`] writes it.
        origin: String,
        /// The row's line in its file.
        line: usize,
    },
    /// No row judges it yet.
    Held,
}

impl fmt::Display for Withheld {
    /// Reads as a note says it: `review looked_at rejected t1w[sub=04]
    /// (qc/looked_at.csv line 2): ghosting`, or `review looked_at holds back
    /// t1w[sub=06]: no verdict yet`.
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

/// Rows of a review file that match no artifact the review judges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnmatchedVerdicts {
    pub file: String,
    /// The lines of those rows in `file`, in order.
    pub lines: Vec<usize>,
}

impl fmt::Display for UnmatchedVerdicts {
    /// Reads as `12 verdicts in qc/looked_at.csv match no artifact (lines
    /// 6–17)`; separate runs of lines are listed, as `lines 3, 6–8`.
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
