//! SPIT's own words, such as `source`, `@vary(...)` and `{@entities}`,
//! where a document writes them, for an editor to explain on hover. Each
//! word's documentation is one entry of a static table, in `docs`. A
//! document is scanned once, line by line, into small records that name
//! their entry, so the text is written once however often a word is used.
//! Names a document defines are explained by `editor`.

use std::ops::Range;

use crate::json::Json;
use crate::parser::{without_bom, Block, Header, Keyword, Layout, Line};
use crate::span::utf16_columns;

mod docs;

pub use docs::{Doc, Word, DOCS, REFERENCE};

/// One use of a word: its 1-based line and 1-based UTF-16 columns, the end
/// exclusive, as in diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WordUse {
    pub line: usize,
    pub columns: Range<usize>,
    pub word: Word,
}

/// Every word `text` uses, in order. Text in a comment uses none.
pub fn builtin_words(text: &str) -> Vec<WordUse> {
    let bom = usize::from(text.starts_with('\u{feff}'));
    let mut uses = Vec::new();
    let mut found = Vec::new();
    let layout = Layout::new(without_bom(text));
    // For each line that a statement's keyword covers without being on it,
    // that keyword's word: an entry of a keyword block, such as `source:`,
    // and each line after a statement's first, such as a command's.
    let mut inherited = vec![None; layout.lines.len()];
    for statement in &layout.statements {
        let block = statement
            .parent
            .and_then(|parent| match layout.statements[parent].opens {
                Some(Block::Keyword(keyword)) => keyword_word(keyword),
                _ => None,
            });
        let first = statement.lines.start;
        inherited[first] = block;
        let own = layout.lines[first].keyword().and_then(keyword_word);
        inherited[first + 1..statement.lines.end].fill(own.or(block));
    }
    for (index, read) in layout.lines.iter().enumerate() {
        let line = read.text;
        found.clear();
        line_words(read, inherited[index], &mut found);
        found.sort_unstable_by_key(|(bytes, _)| bytes.start);
        let shift = 1 + if index == 0 { bom } else { 0 };
        uses.extend(found.iter().map(|(bytes, word)| {
            let units = utf16_columns(line, bytes);
            WordUse {
                line: index + 1,
                columns: units.start + shift..units.end + shift,
                word: *word,
            }
        }));
    }
    uses
}

/// `check --json`'s `words`, each use naming its word, and `word_docs`,
/// each word used given once.
pub(crate) fn words_json(uses: &[WordUse]) -> [(&'static str, Json<'static>); 2] {
    let mut used = [false; DOCS.len()];
    for word_use in uses {
        used[word_use.word as usize] = true;
    }
    let occurrences = Json::array(uses.iter().map(|word_use| {
        Json::object([
            ("line", Json::Number(word_use.line)),
            ("column", Json::Number(word_use.columns.start)),
            ("end_column", Json::Number(word_use.columns.end)),
            ("word", Json::string(word_use.word.doc().name)),
        ])
    }));
    let docs = Json::object(
        DOCS.iter()
            .zip(used)
            .filter(|(_, used)| *used)
            .map(|(doc, _)| {
                (
                    doc.name,
                    Json::object([
                        ("kind", Json::string(doc.kind)),
                        ("example", Json::string(doc.example)),
                        ("summary", Json::string(doc.summary)),
                        (
                            "reference",
                            Json::String(format!("{REFERENCE}#{}", doc.anchor).into()),
                        ),
                    ]),
                )
            }),
    );
    [("words", occurrences), ("word_docs", docs)]
}

/// The word of a statement that starts with `keyword`, if SPIT documents
/// one.
fn keyword_word(keyword: Keyword) -> Option<Word> {
    Some(match keyword {
        Keyword::Use => Word::Use,
        Keyword::Root => Word::Root,
        Keyword::Source => Word::Source,
        Keyword::Operation => Word::Operation,
        Keyword::Command => Word::Command,
        Keyword::Verify => Word::Verify,
        Keyword::Check | Keyword::Checks => Word::Check,
        Keyword::Require => Word::Require,
        Keyword::Exclude => Word::Exclude,
        Keyword::Path => Word::Path,
        Keyword::Ext => Word::Ext,
        Keyword::Default => Word::Default,
        Keyword::Stage => Word::Stage,
        Keyword::Dimensions => Word::Dimensions,
        Keyword::Types => Word::Types,
        Keyword::Type => Word::Type,
        Keyword::Entities => Word::EntitiesFormat,
        Keyword::Drop | Keyword::Sidecars | Keyword::Skip | Keyword::ShellSource => return None,
    })
}

/// The statement `line` starts with, as SPIT's parser classifies it,
/// and the length of its keyword.
fn statement(line: &Line) -> Option<(Word, usize)> {
    if let Some(keyword) = line.keyword() {
        let word = keyword_word(keyword)?;
        let length = if word == Word::Ext {
            3
        } else {
            word.doc().name.len()
        };
        return Some((word, length));
    }
    // A recipe's or `.spitout`'s header lines, as `inputs` reads them.
    [(Word::Pipeline, "pipeline "), (Word::Root, "root ")]
        .into_iter()
        .find(|(_, prefix)| line.code.starts_with(prefix) && !line.code.contains('='))
        .map(|(word, prefix)| (word, prefix.len() - 1))
}

/// Each word `line` uses, by byte range.
/// The words `line` uses, its statement's word being `inherited` when the
/// keyword that gives it is not on the line.
fn line_words(line: &Line, inherited: Option<Word>, found: &mut Vec<(Range<usize>, Word)>) {
    let code = line.uncommented;
    let indent = line.indent;
    let trimmed = code.trim();
    if trimmed == "inputs:" {
        found.push((indent..indent + "inputs:".len(), Word::Inputs));
        return;
    }
    if indent > 0 && trimmed.starts_with("contexts: ") {
        found.push((indent..indent + "contexts".len(), Word::ContextEntry));
    }
    if let Some(header) = line.header() {
        let (word, length) = match header {
            Header::Sources => (Word::Sources, "sources".len()),
            Header::SourcePaths => (Word::SourcePaths, "source_paths".len()),
            Header::Contexts(_) => (Word::Contexts, "contexts".len()),
            Header::Removed => (Word::Removed, "removed".len()),
        };
        found.push((indent..indent + length, word));
        return;
    }
    // The statement's word, the offset it starts at and its length. After
    // `default`, the setting is read as its own line would be.
    let mut statement = statement(line).map(|(word, length)| (word, indent, length));
    if let Some((Word::Default, at, length)) = statement {
        found.push((at..at + length, Word::Default));
        let rest = &code[at + length..];
        let setting = code.len() - rest.trim_start().len();
        statement = self::statement(&Line::new(&code[setting..]))
            .map(|(word, length)| (word, setting, length));
    }
    if let Some((word, at, length)) = statement {
        found.push((at..at + length, word));
    }
    let kind = statement.map(|(word, _, _)| word).or(inherited);
    placeholders(code, kind, found);
    if kind == Some(Word::Command) {
        redirections(code, found);
    }
    let bytes = code.as_bytes();
    let mut start = statement.map_or(indent, |(_, at, length)| at + length);
    while start < bytes.len() {
        if !(bytes[start].is_ascii_alphabetic() || bytes[start] == b'_') {
            start += 1;
            continue;
        }
        let end = bytes[start..]
            .iter()
            .position(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
            .map_or(bytes.len(), |length| start + length);
        // A word continues a name across `.`, `-` or `::`, as in a file
        // name or an imported product, so only a whole word counts.
        let whole = start == 0 || !matches!(bytes[start - 1], b'.' | b'-' | b':' | b'/');
        if whole {
            if let Some(word) = rule_word(code, start..end, kind) {
                found.push((start..end, word));
            }
        }
        start = end;
    }
}

/// The word `code[range]` is in a line that starts with `statement`, if one.
fn rule_word(code: &str, range: Range<usize>, statement: Option<Word>) -> Option<Word> {
    let text = &code[range.clone()];
    let before = code[..range.start].trim_end();
    let tight = code[..range.start].ends_with('@');
    let spaced = code[range.end..].starts_with(' ');
    let after = code[range.end..].trim_start();
    let selector = match text {
        "vary" => Some(Word::Vary),
        "each" => Some(Word::Each),
        "where" => Some(Word::Where),
        "same" => Some(Word::Same),
        "min" => Some(Word::Min),
        "check" => Some(Word::CheckClause),
        _ => None,
    };
    if let Some(word) = selector {
        // Keep in step with `no_space_after_at` in `parser/check.rs`: a clause
        // is `@keyword(`, with nothing between the `@` and the keyword.
        if tight && after.starts_with('(') {
            return Some(word);
        }
    }
    match (statement?, text) {
        (Word::Operation, "many") if before.ends_with(':') => Some(Word::Many),
        (Word::Operation, "beside") if before.contains("->") => Some(Word::Beside),
        (Word::Source, "beside") if before.split_whitespace().count() > 1 && spaced => {
            Some(Word::Beside)
        }
        (Word::Use, "as") if spaced => Some(Word::UseAs),
        (Word::Use, "from") if spaced => Some(Word::UseFrom),
        (Word::Exclude, "from") if spaced && before.trim_start() == "exclude" => {
            Some(Word::ExcludeFrom)
        }
        (Word::Require | Word::Exclude, "count") if after.starts_with(['=', '!', '>', '<']) => {
            Some(Word::Count)
        }
        (Word::Require, "where") if before.ends_with(']') => Some(Word::RequireWhere),
        (Word::Require, "has") if before.contains(" where ") => Some(Word::RequireHas),
        (Word::Exclude, "where") if before.ends_with(']') => Some(Word::ExcludeWhere),
        (Word::Exclude, "missing") if before.contains(" where ") => Some(Word::Missing),
        (Word::Exclude, "has") if before.contains(" where ") => Some(Word::Has),
        _ => None,
    }
}

/// The built-in placeholders in `code`: SPIT's own `{@...}` anywhere, and
/// in a command, `{@output}` and an output's `.dir` and `.stem`. `{{` and
/// `}}` are literal braces.
fn placeholders(code: &str, statement: Option<Word>, found: &mut Vec<(Range<usize>, Word)>) {
    let in_command = matches!(statement, Some(Word::Command | Word::Verify));
    let in_check = statement == Some(Word::Check);
    let bytes = code.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"{{") || bytes[index..].starts_with(b"}}") {
            index += 2;
            continue;
        }
        if bytes[index] != b'{' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let Some(length) = bytes[start..]
            .iter()
            .position(|byte| matches!(byte, b'{' | b'}'))
        else {
            return;
        };
        let end = start + length;
        if bytes[end] == b'{' {
            index = end;
            continue;
        }
        let name = &code[start..end];
        let word = match name {
            "@product" => Some(Word::Product),
            "@entities" => Some(Word::Entities),
            "@stage" => Some(Word::StageName),
            "@output" if in_command => Some(Word::Output),
            "@path" if in_check => Some(Word::PathPlaceholder),
            _ => None,
        };
        if let Some(word) = word {
            found.push((start..end, word));
        } else if !in_command && !in_check {
            // A shape after the `:` of a dimension, as in `{date:date}`.
            if let Some((dimension, shape)) = name.split_once(':') {
                let word = match shape {
                    "digits" => Some(Word::ShapeDigits),
                    "year" => Some(Word::ShapeYear),
                    "date" => Some(Word::ShapeDate),
                    _ => None,
                };
                if let (Some(word), false) = (word, dimension.starts_with('@')) {
                    found.push((end - shape.len()..end, word));
                }
            }
        } else if in_command {
            for (suffix, word) in [(".dir", Word::Dir), (".stem", Word::Stem)] {
                if name.len() > suffix.len() && name.ends_with(suffix) {
                    found.push((end - suffix.len()..end, word));
                }
            }
        }
        index = end + 1;
    }
}

/// Each `<` and `>` in a command's `code` that is a word of its own, outside
/// quotes, as the command's parser reads it.
fn redirections(code: &str, found: &mut Vec<(Range<usize>, Word)>) {
    let bytes = code.as_bytes();
    let mut quote = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match (quote, byte) {
            (None, b'\\') | (Some(b'"'), b'\\') => index += 1,
            (None, b'\'' | b'"') => quote = Some(byte),
            (Some(open), _) if byte == open => quote = None,
            (None, b'<' | b'>') => {
                let apart = |at: Option<&u8>| at.is_none_or(u8::is_ascii_whitespace);
                let before = index.checked_sub(1).map(|at| &bytes[at]);
                if apart(before) && apart(bytes.get(index + 1)) {
                    let word = if byte == b'<' {
                        Word::Stdin
                    } else {
                        Word::Stdout
                    };
                    found.push((index..index + 1, word));
                }
            }
            _ => {}
        }
        index += 1;
    }
}
