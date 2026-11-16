//! A file written beside another.

use std::fmt;

/// A file written beside the named port's or source's file. Its name is
/// that file's without its extension, then `suffix` (`.json`, or
/// `_mask.nii.gz`), or with `append`, that file's whole name, then `suffix`
/// (`genome.fasta.fai`). The suffix is a naming part, `_mask`, then the
/// extension, `.nii.gz`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Beside {
    pub sibling: String,
    /// What the file's name ends with: `name`, then its extension.
    pub suffix: String,
    /// The part of `suffix` before the extension, as `_mask`; empty when the
    /// name is the sibling's, as for `.json`.
    pub name: String,
    /// Whether the suffix follows the whole name, as `+.fai` writes, rather
    /// than replacing the extension.
    pub append: bool,
}

/// Where a product written beside another lies, as
/// [`Pipeline::beside`](super::Pipeline::beside) gives it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BesideFile<'p> {
    /// The product whose file this one lies beside.
    pub sibling: &'p str,
    /// The sibling's extension, which `suffix` replaces; `None` when the
    /// suffix follows the whole name, or when the sibling has no extension
    /// to replace.
    pub replaces: Option<&'p str>,
    pub suffix: &'p str,
    pub append: bool,
    /// For an output written beside one of its step's inputs, that input's
    /// place among the operation's inputs; `None` beside an output or for
    /// a source.
    pub input: Option<usize>,
}

impl Beside {
    /// The extension a file written so has: its suffix after the naming part.
    pub fn extension(&self) -> Option<&str> {
        self.suffix
            .get(self.name.len()..)
            .filter(|ext| !ext.is_empty())
    }
}

impl fmt::Display for Beside {
    /// Reads as written: `+.fai beside fasta`, or `.nii.gz beside image
    /// suffix "_mask"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.append {
            f.write_str("+")?;
        }
        write!(
            f,
            "{} beside {}",
            self.extension().unwrap_or_default(),
            self.sibling
        )?;
        if !self.name.is_empty() {
            write!(f, " suffix \"{}\"", self.name)?;
        }
        Ok(())
    }
}
