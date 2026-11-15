//! What a job's tools are: the names a pipeline gives the programs its
//! operations run, which jobs depend on, and what a `.spitenv` says each one
//! is on the machine the jobs run on.

/// A `uses` list, as written: the tools it adds, and those it removes with
/// `!`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolList {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// A `uses operation: tools` line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsesDef {
    pub operation: String,
    pub tools: ToolList,
}

/// The tools each job of an operation depends on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolSet {
    /// Every tool the `.spitenv` names, but these: what an operation gets
    /// when no list it inherits names a tool to add.
    Every { except: Vec<String> },
    /// These tools, in the order they were added.
    Only(Vec<String>),
}

impl Default for ToolSet {
    fn default() -> Self {
        Self::Every { except: Vec::new() }
    }
}

impl ToolSet {
    /// This set with `list` applied: its first name to add turns every tool
    /// into a list of its own, its other names add to that list, and each
    /// `!` name is removed from either.
    #[must_use]
    pub fn with(self, list: &ToolList) -> Self {
        let mut set = match self {
            Self::Every { .. } if !list.add.is_empty() => Self::Only(Vec::new()),
            set => set,
        };
        match &mut set {
            Self::Every { except } => {
                for name in &list.remove {
                    if !except.contains(name) {
                        except.push(name.clone());
                    }
                }
            }
            Self::Only(names) => {
                for name in &list.add {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
                names.retain(|name| !list.remove.contains(name));
            }
        }
        set
    }

    /// Whether a job of this set depends on the tool `name`, when it is one
    /// the `.spitenv` names.
    pub fn includes(&self, name: &str) -> bool {
        match self {
            Self::Every { except } => !except.iter().any(|other| other == name),
            Self::Only(names) => names.iter().any(|other| other == name),
        }
    }

    /// Every name the set holds, added or excepted.
    pub fn names(&self) -> &[String] {
        match self {
            Self::Every { except } => except,
            Self::Only(names) => names,
        }
    }
}

/// What a `.spitenv` says one tool is on this machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tool {
    pub name: String,
    pub identity: ToolIdentity,
}

/// How a tool is identified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolIdentity {
    /// A text written in the `.spitenv`, such as a container's digest,
    /// compared as it is.
    Declared(String),
    /// `probe program args...`: a command, run with no shell, whose output
    /// is the tool's identity and whose failure stops a run.
    Probe(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::{ToolList, ToolSet};

    fn list(add: &[&str], remove: &[&str]) -> ToolList {
        ToolList {
            add: add.iter().map(|name| (*name).to_owned()).collect(),
            remove: remove.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    #[test]
    fn the_first_name_replaces_every_tool_and_later_names_add_up() {
        let set = ToolSet::default()
            .with(&list(&["mrtrix3"], &[]))
            .with(&list(&["fsl"], &[]));
        assert_eq!(set, ToolSet::Only(vec!["mrtrix3".into(), "fsl".into()]));
        let set = set.with(&list(&[], &["mrtrix3"]));
        assert_eq!(set, ToolSet::Only(vec!["fsl".into()]));
    }

    #[test]
    fn a_removal_alone_keeps_every_other_tool() {
        let set = ToolSet::default().with(&list(&[], &["fsl"]));
        assert!(!set.includes("fsl"));
        assert!(set.includes("mrtrix3"));
        let set = set.with(&list(&["ants"], &[]));
        assert_eq!(set, ToolSet::Only(vec!["ants".into()]));
    }
}
