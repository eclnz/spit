use std::fmt;

use crate::template::CommandTemplate;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DefaultChecks {
    pub checks: Vec<CheckUse>,
    pub exempt: Vec<CheckUse>,
    pub by_extension: Vec<ExtensionChecks>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionChecks {
    pub extension: String,
    pub checks: Vec<CheckUse>,
    pub exempt: Vec<CheckUse>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DefaultReviews {
    pub reviews: Vec<String>,
    pub exempt: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CheckKind {
    #[default]
    Check,
    Screen,
}

impl CheckKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Screen => "screen",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckDef {
    pub kind: CheckKind,
    pub name: String,
    pub parameters: Vec<String>,
    pub template: CommandTemplate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckUse {
    pub kind: CheckKind,
    pub check: String,
    pub arguments: Vec<String>,
}

impl fmt::Display for CheckUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.check)?;
        if !self.arguments.is_empty() {
            write!(f, "({})", self.arguments.join(", "))?;
        }
        Ok(())
    }
}
