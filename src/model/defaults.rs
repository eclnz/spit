//! The default `check:` and `review:` lists of a file or stage.

use super::CheckUse;

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
