use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

fn quoted_end(bytes: &[u8], mut index: usize, quote: u8) -> usize {
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == quote => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let hashes = bytes[start + 1..]
        .iter()
        .take_while(|&&b| b == b'#')
        .count();
    let quote = start + 1 + hashes;
    if bytes.get(quote) != Some(&b'"') {
        return None;
    }
    let delimiter = &bytes[start + 1..quote];
    for (offset, &byte) in bytes[quote + 1..].iter().enumerate() {
        let index = quote + 1 + offset;
        if byte == b'"' && bytes[index + 1..].starts_with(delimiter) {
            return Some(index + 1 + hashes);
        }
    }
    Some(bytes.len())
}

fn block_comment_end(bytes: &[u8], mut index: usize) -> usize {
    let mut depth = 1;
    while index + 1 < bytes.len() {
        match &bytes[index..index + 2] {
            b"/*" => {
                depth += 1;
                index += 2;
            }
            b"*/" => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    bytes.len()
}

fn comments(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut index = 0;
    if source.starts_with("#!/") {
        index = source.find('\n').unwrap_or(bytes.len());
    }
    while index < bytes.len() {
        let start = index;
        match bytes[index] {
            b'/' if bytes[index..].starts_with(b"//") => {
                index = source[index..]
                    .find('\n')
                    .map_or(bytes.len(), |end| index + end);
                found.push(start..index);
            }
            b'/' if bytes[index..].starts_with(b"/*") => {
                index = block_comment_end(bytes, index + 2);
                found.push(start..index);
            }
            b'r' => index = raw_string_end(bytes, index).unwrap_or(index + 1),
            b'"' => index = quoted_end(bytes, index + 1, b'"'),
            b'\'' if bytes.get(index + 1) == Some(&b'\\') => {
                index = quoted_end(bytes, index + 1, b'\'');
            }
            b'\'' => {
                let next = index + 1;
                let after_char = source[next..]
                    .chars()
                    .next()
                    .map_or(next, |c| next + c.len_utf8());
                index = if bytes.get(after_char) == Some(&b'\'') {
                    after_char + 1
                } else {
                    next
                };
            }
            _ => index += 1,
        }
    }
    found
}

fn rust_files(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn rust_sources_have_no_comments() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for folder in ["src", "tests", "examples", "benches"] {
        let path = root.join(folder);
        if path.is_dir() {
            rust_files(&path, &mut files);
        }
    }
    let build_script = root.join("build.rs");
    if build_script.is_file() {
        files.push(build_script);
    }
    files.sort();

    let mut violations = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).unwrap();
        for comment in comments(&source) {
            let line = source[..comment.start]
                .bytes()
                .filter(|&b| b == b'\n')
                .count()
                + 1;
            violations.push(format!(
                "{}:{line}: Rust comments are forbidden",
                file.strip_prefix(root).unwrap().display()
            ));
        }
    }
    assert!(violations.is_empty(), "\n{}", violations.join("\n"));
}

#[test]
fn distinguishes_comments_from_literals_and_lifetimes() {
    let source = r####"#!/usr/bin/env rust-script
let _ = ("https://example.com", "escaped \" /* text */", r##""# // text /* still text */"##);
let _ = (b"//", br"/*", c"//", cr#"/*"#, '\'', 'é', b'"', '/');
fn example<'a>() { 'label: loop { break 'label; } }
/// Documentation
let/* nested /* comment */ ends */_ = 1; // Inline
"####;
    let found: Vec<_> = comments(source)
        .iter()
        .map(|span| &source[span.clone()])
        .collect();
    assert_eq!(
        found,
        [
            "/// Documentation",
            "/* nested /* comment */ ends */",
            "// Inline",
        ]
    );
}
