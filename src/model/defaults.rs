//! The default `check:` and `review:` lists of a file or stage.

use std::fmt;

use crate::template::CommandTemplate;

/// A `check:` line: the checks every output of a file or stage runs, and
/// the checks of the scope around it that its outputs do not.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DefaultChecks {
    pub checks: Vec<CheckUse>,
    pub exempt: Vec<CheckUse>,
    /// The lists written for one extension, as `default check .nii.gz:
    /// nifti_ok`, in the order written. Each covers the files of its scope
    /// whose whole extension it names, sources included.
    pub by_extension: Vec<ExtensionChecks>,
}

/// A `default check .ext:` list: the checks of the files with that
/// extension, and the checks of the scope around it that they do not run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionChecks {
    /// The whole extension, such as `.nii.gz`.
    pub extension: String,
    pub checks: Vec<CheckUse>,
    pub exempt: Vec<CheckUse>,
}

/// A `review:` line: the reviews that judge every output of a file or
/// stage, and the reviews of the scope around it that its outputs are not
/// judged by.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DefaultReviews {
    pub reviews: Vec<String>,
    pub exempt: Vec<String>,
}

/// What a command attached to an artifact decides.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CheckKind {
    /// A fault fails the work.
    #[default]
    Check,
    /// A nonzero verdict excludes the artifact.
    Screen,
}

impl CheckKind {
    /// The keyword for this decision.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Screen => "screen",
        }
    }
}

/// A test of one artifact, declared once as `check name(params): command`
/// and attached where it applies with `@check(name(arguments))`. Its
/// command reads `{@path}`, the artifact, and each `{param}`, the text the
/// attachment gives; a nonzero exit fails the job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckDef {
    pub kind: CheckKind,
    pub name: String,
    pub parameters: Vec<String>,
    pub template: CommandTemplate,
}

/// A check attached to a port or a source, with the text it gives each of
/// the check's parameters, as in `ndim(3)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckUse {
    pub kind: CheckKind,
    pub check: String,
    pub arguments: Vec<String>,
}

/// Reads as written: `nonempty`, or `ndim(3)` with arguments.
impl fmt::Display for CheckUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.check)?;
        if !self.arguments.is_empty() {
            write!(f, "({})", self.arguments.join(", "))?;
        }
        Ok(())
    }
}
