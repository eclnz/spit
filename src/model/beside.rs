use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Beside {
    pub sibling: String,
    pub suffix: String,
    pub name: String,
    pub append: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BesideFile<'p> {
    pub sibling: &'p str,
    pub replaces: Option<&'p str>,
    pub suffix: &'p str,
    pub append: bool,
    pub input: Option<usize>,
}

impl Beside {
    pub fn extension(&self) -> Option<&str> {
        self.suffix
            .get(self.name.len()..)
            .filter(|ext| !ext.is_empty())
    }
}

impl fmt::Display for Beside {
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
