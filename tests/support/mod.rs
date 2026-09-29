use std::fs;
use std::path::{Path, PathBuf};

pub struct Tree(pub PathBuf);

impl Tree {
    pub fn new(name: &str, files: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!("spit-tree-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        Self(root)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
