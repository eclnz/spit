//! The `spit` code blocks in the user pages check. A block is a pipeline that
//! `spit check` accepts, unless a comment before it says otherwise. The
//! comment is hidden from readers, and starts `<!-- spit-doc: KIND`:
//!
//! - `fragment`, alone on its line: the block is not a whole file, and is not
//!   checked. It must not check on its own, or the marker is stale.
//! - `setup`: the lines of the comment are put before the block, and the
//!   whole is checked as a pipeline. The block must not check on its own.
//! - `recipe`: the same, but the whole is checked as a `.spitin` recipe.
//! - `files`: the block holds several files, each opened by a `# name.spit` or
//!   `# name.spitin` line, and each is checked.
//!
//! In a `setup` or `recipe` comment, a line `--- name` opens a file written
//! beside the block, which the lines after it hold, such as the pipeline a
//! recipe names; `--- name = path` copies the repository's file `path`. The
//! lines before the first `---` go before the block.

mod support;

use std::fs;
use std::path::Path;
use std::process::Command;

use support::Tree;

const OPEN: &str = "<!-- spit-doc:";

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

#[derive(Debug, PartialEq)]
enum Kind {
    Fragment,
    Setup,
    Recipe,
    Files,
}

/// The comment before a block.
struct Marker {
    kind: Kind,
    /// Lines before the first `---`, which go before the block.
    head: String,
    /// The files beside the block: a name, and its text or the path to copy.
    files: Vec<(String, Source)>,
}

enum Source {
    Text(String),
    Copy(String),
}

struct Block {
    line: usize,
    text: String,
    marker: Option<Marker>,
}

fn parse_marker(comment: &[&str]) -> Marker {
    let first = comment[0].trim_start().trim_start_matches(OPEN).trim();
    let word = first.trim_end_matches("-->").trim();
    let kind = match word {
        "fragment" => Kind::Fragment,
        "setup" => Kind::Setup,
        "recipe" => Kind::Recipe,
        "files" => Kind::Files,
        other => panic!("unknown spit-doc kind `{other}`"),
    };
    // A comment of several lines ends with a line that closes it.
    let body = if comment.len() > 1 {
        &comment[1..comment.len() - 1]
    } else {
        &[]
    };
    let mut head = String::new();
    let mut files: Vec<(String, Source)> = Vec::new();
    for line in body {
        if let Some(spec) = line.strip_prefix("--- ") {
            files.push(match spec.split_once(" = ") {
                Some((name, path)) => {
                    (name.trim().to_owned(), Source::Copy(path.trim().to_owned()))
                }
                None => (spec.trim().to_owned(), Source::Text(String::new())),
            });
        } else if let Some((_, Source::Text(text))) = files.last_mut() {
            *text += line;
            text.push('\n');
        } else {
            head += line;
            head.push('\n');
        }
    }
    Marker { kind, head, files }
}

/// Each ```` ```spit ```` block of `text`, with the marker comment before it.
fn spit_blocks(text: &str) -> Vec<Block> {
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
                blocks.push(Block {
                    line: opening + 1,
                    text: body,
                    marker: marker_before(&lines, opening),
                });
            }
        }
        index += 1;
    }
    blocks
}

/// The `<!-- spit-doc: ... -->` comment that ends on the last non-blank line
/// before `opening`, if there is one.
fn marker_before(lines: &[&str], opening: usize) -> Option<Marker> {
    let end = (0..opening).rev().find(|&i| !lines[i].trim().is_empty())?;
    if !lines[end].trim_end().ends_with("-->") {
        return None;
    }
    let start = (0..=end)
        .rev()
        .find(|&i| lines[i].trim_start().starts_with(OPEN))?;
    if lines[start..=end]
        .iter()
        .skip(1)
        .any(|line| line.contains("<!--"))
    {
        return None;
    }
    Some(parse_marker(&lines[start..=end]))
}

fn checks(tree: &Tree, file: &Path) -> Result<(), String> {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .arg("check")
        .arg(tree.path().join(file))
        .output()
        .unwrap();
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

fn write_files(tree: &Tree, marker: &Marker) {
    for (name, source) in &marker.files {
        match source {
            Source::Text(text) => tree.write(name, text),
            Source::Copy(path) => tree.write(name, &fs::read_to_string(path).unwrap()),
        };
    }
}

/// Why `block` fails its check, if it does.
fn verdict(page: &str, block: &Block) -> Option<String> {
    let at = format!("{page}:{}", block.line);
    let tree = Tree::new("docs-examples", &[]);
    let alone = |text: &str| {
        tree.write("block.spit", text);
        checks(&tree, Path::new("block.spit"))
    };
    let Some(marker) = &block.marker else {
        return alone(&block.text).err().map(|error| {
            format!("{at}: does not check; fix it, or mark it with `{OPEN} fragment -->`\n{error}")
        });
    };
    match marker.kind {
        Kind::Fragment => alone(&block.text)
            .is_ok()
            .then(|| format!("{at}: is marked as a fragment but checks alone; remove the marker")),
        Kind::Setup | Kind::Recipe => {
            if marker.kind == Kind::Setup && alone(&block.text).is_ok() {
                return Some(format!("{at}: checks alone; remove its setup"));
            }
            write_files(&tree, marker);
            let name = if marker.kind == Kind::Setup {
                "whole.spit"
            } else {
                "whole.spitin"
            };
            tree.write(name, &format!("{}{}", marker.head, block.text));
            checks(&tree, Path::new(name))
                .err()
                .map(|error| format!("{at}: does not check with its setup\n{error}"))
        }
        Kind::Files => {
            let mut files: Vec<(String, String)> = Vec::new();
            for line in block.text.lines() {
                let name = line
                    .strip_prefix("# ")
                    .filter(|name| name.ends_with(".spit") || name.ends_with(".spitin"));
                match (name, files.last_mut()) {
                    (Some(name), _) => files.push((name.to_owned(), String::new())),
                    (None, Some((_, text))) => {
                        *text += line;
                        text.push('\n');
                    }
                    (None, None) => {}
                }
            }
            if files.is_empty() {
                return Some(format!(
                    "{at}: marked `files` but holds no `# name.spit` line"
                ));
            }
            for (name, text) in &files {
                tree.write(name, text);
            }
            files.iter().find_map(|(name, _)| {
                checks(&tree, Path::new(name))
                    .err()
                    .map(|error| format!("{at}: `{name}` does not check\n{error}"))
            })
        }
    }
}

#[test]
fn every_spit_block_in_the_user_pages_checks_or_says_why_it_does_not() {
    let mut problems = Vec::new();
    for page in user_pages() {
        let text = fs::read_to_string(&page).unwrap();
        for block in spit_blocks(&text) {
            problems.extend(verdict(&page, &block));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
