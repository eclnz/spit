//! A path written with `*`, as `exclude path` and `prefer path` name the
//! files and folders discovery found.

use std::fmt;

use super::path::{PathPart, PathTemplate};
use super::pattern::PathPattern;

/// A path under the dataset root, as text and `*`, with `**` for a literal
/// star. It covers a found path when it matches that path's first folders,
/// as many as it has, so a folder covers every file and folder under it.
#[derive(Clone, Debug)]
pub struct PathGlob {
    template: PathTemplate,
    pattern: PathPattern,
    /// How many folder or file names it has, which `*` never changes.
    depth: usize,
}

impl PathGlob {
    /// `text`, without a trailing `/`, or why it is not a path of text and
    /// `*` under the root.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim().trim_end_matches('/');
        if text.is_empty() {
            return Err("a path under the dataset root is missing".to_owned());
        }
        let template = PathTemplate::parse(text).map_err(|error| error.to_string())?;
        let mut depth = 1;
        for part in template.parts() {
            match part {
                PathPart::Literal(literal) => depth += literal.matches('/').count(),
                PathPart::Wildcard => {}
                PathPart::Placeholder(_) | PathPart::Group(_) | PathPart::Fixed { .. } => {
                    return Err(format!(
                        "`{text}` reads values, but a path here is text and `*`, with `**` for a literal star"
                    ));
                }
            }
        }
        let pattern = PathPattern::new(&template, "", None);
        Ok(Self {
            template,
            pattern,
            depth,
        })
    }

    /// Whether it covers `found`, a path under the dataset root: the path
    /// itself, or a folder that holds it.
    pub(crate) fn covers(&self, found: &str) -> bool {
        let end = match found.match_indices('/').nth(self.depth - 1) {
            Some((end, _)) => end,
            None if found.matches('/').count() + 1 == self.depth => found.len(),
            None => return false,
        };
        self.pattern.match_path(&found[..end]).is_some()
    }
}

/// Two globs are the same when they are written the same.
impl PartialEq for PathGlob {
    fn eq(&self, other: &Self) -> bool {
        self.template == other.template
    }
}

impl Eq for PathGlob {}

/// Reads as written, as `ProjA_Case_*`.
impl fmt::Display for PathGlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.template.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::PathGlob;

    fn glob(text: &str) -> PathGlob {
        PathGlob::parse(text).expect("a valid glob")
    }

    #[test]
    fn a_folder_covers_what_is_under_it() {
        let glob = glob("Proj_A_4_A");
        assert!(glob.covers("Proj_A_4_A"));
        assert!(glob.covers("Proj_A_4_A/Proj A - 1234/anat_T1_9"));
        assert!(!glob.covers("Proj_A_4_AB/x"));
    }

    #[test]
    fn a_star_matches_within_one_name() {
        let glob = glob("ProjA_Case_*");
        assert!(glob.covers("ProjA_Case_4_A/Proj A - 1234/anat_T1_9"));
        assert!(!glob.covers("Proj_A_4_A/ProjA_Case_4/anat_T1_9"));
        let session = self::glob("*/* - *");
        assert!(session.covers("Proj_A_6_B/Proj A - 2187/anat_T1_4"));
        assert!(!session.covers("Proj_A_6_B/Proj_A_6_B/anat_T1_4"));
    }

    #[test]
    fn two_stars_are_a_literal_star() {
        assert!(glob("a**b").covers("a*b/c"));
        assert!(!glob("a**b").covers("axb/c"));
    }

    #[test]
    fn placeholders_and_empty_paths_are_rejected() {
        for text in ["sub-{sub}", "a/[{x}]", "", "x/{v=1}"] {
            assert!(PathGlob::parse(text).is_err(), "{text}");
        }
    }
}
