#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolList {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsesDef {
    pub operation: String,
    pub tools: ToolList,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolSet {
    Every { except: Vec<String> },
    Only(Vec<String>),
}

impl Default for ToolSet {
    fn default() -> Self {
        Self::Every { except: Vec::new() }
    }
}

impl ToolSet {
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

    pub fn includes(&self, name: &str) -> bool {
        match self {
            Self::Every { except } => !except.iter().any(|other| other == name),
            Self::Only(names) => names.iter().any(|other| other == name),
        }
    }

    pub fn names(&self) -> &[String] {
        match self {
            Self::Every { except } => except,
            Self::Only(names) => names,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tool {
    pub name: String,
    pub identity: ToolIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolIdentity {
    Declared(String),
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
