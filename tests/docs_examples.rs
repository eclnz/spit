//! The `spit` code blocks in the user pages check: each is a pipeline that
//! `spit check` accepts, or is marked as not meant to run alone. A marker is
//! a comment on the line before the block, `<!-- spit-doc: fragment -->`, which
//! no page shows.

mod support;

use std::fs;
use std::path::Path;
use std::process::Command;

use support::Tree;

const MARKER: &str = "<!-- spit-doc: fragment -->";

/// The pages written for people writing pipelines.
fn user_pages() -> Vec<String> {
    let mut pages = vec!["README.md".to_owned()];
    for folder in ["docs", "docs/guide", "docs/reference"] {
        let mut found: Vec<_> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .map(|path| path.to_str().unwrap().to_owned())
            .collect();
        found.sort();
        pages.extend(found);
    }
    // `docs/architecture.md` is for contributors, as is its folder.
    pages.retain(|page| page != "docs/architecture.md");
    pages
}

/// Each ```` ```spit ```` block of `text` with its line, its text, and
/// whether a marker comment precedes it.
fn spit_blocks(text: &str) -> Vec<(usize, String, bool)> {
    let lines: Vec<_> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].starts_with("```") {
            let opening = index;
            let language = lines[opening].trim_start_matches('`').trim();
            index += 1;
            let mut body = String::new();
            while index < lines.len() && !lines[index].starts_with("```") {
                body += lines[index];
                body.push('\n');
                index += 1;
            }
            if language == "spit" {
                let marked = lines[..opening]
                    .iter()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .is_some_and(|line| line.trim() == MARKER);
                blocks.push((opening + 1, body, marked));
            }
        }
        index += 1;
    }
    blocks
}

fn checks(tree: &Tree, body: &str) -> Result<(), String> {
    let file = tree.write("block.spit", body);
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .arg("check")
        .arg(&file)
        .output()
        .unwrap();
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

#[test]
fn every_spit_block_in_the_user_pages_checks_or_is_marked_as_a_fragment() {
    let tree = Tree::new("docs-examples", &[]);
    let mut problems = Vec::new();
    for page in user_pages() {
        assert!(Path::new(&page).exists(), "{page}");
        let text = fs::read_to_string(&page).unwrap();
        for (line, body, marked) in spit_blocks(&text) {
            match (checks(&tree, &body), marked) {
                (Err(error), false) => problems.push(format!(
                    "{page}:{line}: does not check; fix it, or mark it with `{MARKER}`\n{error}"
                )),
                (Ok(()), true) => problems.push(format!(
                    "{page}:{line}: is marked as a fragment but checks alone; remove the marker"
                )),
                _ => {}
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
