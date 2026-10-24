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
//! A `sh` block must say whether it runs, and a `text` block that shows what a
//! command prints may say so:
//!
//! - `run` before a `sh` block: its lines run in a folder that holds a copy of
//!   `examples/`, with `spit` and `cargo run --` both running the built binary,
//!   and must succeed. A `sh` block with `fragment` before it is not run.
//! - `output` before a `text` block: the lines of the comment before its first
//!   `---` run in that folder, and every line of the block must be part of a
//!   line the command printed, in order, so a block may leave out the `error:
//!   line 3, column 1:` before a message, or show only part of the output.
//! - `spitout` before a `text` block: the block is a `.spitout`, and is checked
//!   as one.
//!
//! In a `setup`, `recipe`, `run` or `output` comment, a line `--- name` opens a file written
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
    Run,
    Output,
    Spitout,
    Plain,
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
    /// The text of the block before this one on the page.
    Previous,
}

struct Block {
    line: usize,
    language: String,
    text: String,
    /// The text of the code block before this one on the page.
    previous: String,
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
        "run" => Kind::Run,
        "output" => Kind::Output,
        "spitout" => Kind::Spitout,
        "plain" => Kind::Plain,
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
                Some((name, "@previous")) => (name.trim().to_owned(), Source::Previous),
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
    let mut previous = String::new();
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
            if ["spit", "sh", "text"].contains(&language) {
                blocks.push(Block {
                    line: opening + 1,
                    language: language.to_owned(),
                    text: body.clone(),
                    previous: previous.clone(),
                    marker: marker_before(&lines, opening),
                });
            }
            previous = body;
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

fn write_files(tree: &Tree, marker: &Marker, previous: &str) {
    for (name, source) in &marker.files {
        match source {
            Source::Text(text) => {
                tree.write(name, text);
            }
            Source::Previous => {
                tree.write(name, previous);
            }
            Source::Copy(path) if Path::new(path).is_dir() => {
                copy_folder(Path::new(path), &tree.path().join(name))
            }
            Source::Copy(path) => {
                tree.write(name, &fs::read_to_string(path).unwrap());
            }
        }
    }
}

/// Copy the folder `from` to `to`, with everything under it.
fn copy_folder(from: &Path, to: &Path) {
    let mut pending = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((from, to)) = pending.pop() {
        fs::create_dir_all(&to).unwrap();
        for entry in fs::read_dir(&from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                pending.push((entry.path(), target));
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
}

/// Why a `spit` block fails its check, if it does.
fn spit_verdict(page: &str, block: &Block) -> Option<String> {
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
            write_files(&tree, marker, &block.previous);
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
        Kind::Run | Kind::Output | Kind::Spitout | Kind::Plain => Some(format!(
            "{at}: `run`, `output`, `spitout` and `plain` are for `sh` and `text` blocks"
        )),
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

/// A folder to run commands in: a copy of `examples/`, the marker's files,
/// and a `bin/` with `spit` and `cargo run --` both running the built binary.
fn command_tree(marker: &Marker, previous: &str) -> Tree {
    let tree = Tree::new("docs-commands", &[]);
    copy_folder(Path::new("examples"), &tree.path().join("examples"));
    write_files(&tree, marker, previous);
    install_shims(&tree);
    tree
}

#[cfg(unix)]
fn install_shims(tree: &Tree) {
    use std::os::unix::fs::PermissionsExt;
    let spit = env!("CARGO_BIN_EXE_spit");
    let shims = [
        ("spit", format!("#!/bin/sh\nexec '{spit}' \"$@\"\n")),
        (
            "cargo",
            "#!/bin/sh\nif [ \"$1\" = run ] && [ \"$2\" = -- ]; then shift 2; exec spit \"$@\"; fi\n\
             echo \"cargo $*: the docs test runs only cargo run --\" >&2\nexit 97\n"
                .to_owned(),
        ),
    ];
    for (name, text) in shims {
        let path = tree.write(&format!("bin/{name}"), &text);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[cfg(not(unix))]
fn install_shims(_: &Tree) {}

/// What `script` prints in `tree`, standard output then standard error, and
/// whether it succeeded.
fn shell(tree: &Tree, script: &str) -> (bool, Vec<String>) {
    let path = format!(
        "{}:{}",
        tree.path().join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("sh")
        .args(["-e", "-c", script])
        .current_dir(tree.path())
        .env("PATH", path)
        .output()
        .unwrap();
    let mut printed = String::from_utf8_lossy(&output.stdout).into_owned();
    printed += &String::from_utf8_lossy(&output.stderr);
    (
        output.status.success(),
        printed
            .lines()
            .map(|line| line.trim_end().to_owned())
            .collect(),
    )
}

/// Whether the first line of `text` that is not a comment starts with one of
/// SPIT's keywords, or is a step, so that the block is SPIT code.
fn looks_like_spit(text: &str) -> bool {
    const KEYWORDS: [&str; 15] = [
        "source",
        "operation",
        "command",
        "stage",
        "use",
        "default",
        "path",
        "verify",
        "check",
        "pipeline",
        "inputs",
        "exclude",
        "require",
        "dimensions",
        "types",
    ];
    let Some(line) = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
    else {
        return false;
    };
    let word: String = line
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let after = line[word.len()..].chars().next();
    if KEYWORDS.contains(&word.as_str()) && matches!(after, None | Some(' ' | ':')) {
        return true;
    }
    // A step: `outputs = operation(`.
    line.split_once(" = ").is_some_and(|(outputs, call)| {
        outputs
            .chars()
            .all(|c| c.is_alphanumeric() || " _:,<>$".contains(c))
            && call.split('(').next().is_some_and(|name| {
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
            })
            && call.contains('(')
    })
}

/// Why a `sh` or `text` block fails, if it does.
fn command_verdict(page: &str, block: &Block) -> Option<String> {
    let at = format!("{page}:{}", block.line);
    let kind = block.marker.as_ref().map(|marker| &marker.kind);
    if cfg!(not(unix)) {
        return None;
    }
    match (block.language.as_str(), kind) {
        ("text", None) if looks_like_spit(&block.text) => Some(format!(
            "{at}: looks like SPIT code in a `text` block; fence it `spit`, or put \
             `{OPEN} fragment -->`, `setup` or `recipe` before it, or `plain` if it is not SPIT"
        )),
        ("text", None) | ("text", Some(Kind::Plain)) => None,
        ("text", Some(Kind::Setup | Kind::Recipe | Kind::Files | Kind::Fragment)) => {
            spit_verdict(page, block)
        }
        ("text", Some(Kind::Output)) => {
            let marker = block.marker.as_ref()?;
            let tree = command_tree(marker, &block.previous);
            let (_, lines) = shell(&tree, &marker.head);
            let mut remaining = lines.iter();
            block
                .text
                .lines()
                .map(str::trim_end)
                .filter(|line| !line.is_empty())
                .find(|line| !remaining.any(|printed| printed.contains(line)))
                .map(|line| {
                    let printed: Vec<_> = lines.iter().take(12).cloned().collect();
                    format!(
                        "{at}: the command does not print `{line}` here, or not in this order\n{}",
                        printed.join("\n")
                    )
                })
        }
        ("text", Some(Kind::Spitout)) => {
            let tree = Tree::new("docs-spitout", &[]);
            tree.write("block.spitout", &block.text);
            checks(&tree, Path::new("block.spitout"))
                .err()
                .map(|error| format!("{at}: does not check as a .spitout\n{error}"))
        }
        ("text", Some(_)) => Some(format!("{at}: a `text` block takes only `{OPEN} output`")),
        ("sh", Some(Kind::Run)) => {
            let marker = block.marker.as_ref()?;
            let tree = command_tree(marker, &block.previous);
            let (succeeded, printed) = shell(&tree, &block.text);
            (!succeeded).then(|| format!("{at}: a command fails\n{}", printed.join("\n")))
        }
        ("sh", Some(Kind::Fragment)) => None,
        _ => Some(format!(
            "{at}: a `sh` block needs `{OPEN} run` or `{OPEN} fragment -->` before it"
        )),
    }
}

/// Why `block` fails its check, if it does.
fn verdict(page: &str, block: &Block) -> Option<String> {
    match block.language.as_str() {
        "spit" => spit_verdict(page, block),
        _ => command_verdict(page, block),
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
