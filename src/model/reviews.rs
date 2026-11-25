use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewDef {
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRecord {
    pub review: String,
    pub file: PathBuf,
}
