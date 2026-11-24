use std::ops::Range;

use crate::json::Json;
use crate::parser::{without_bom, Block, Header, Keyword, Layout, Line};
use crate::span::utf16_columns;
use crate::template::builtin::CHECKED_PATH;
use crate::template::builtin::{CORES, ENTITIES, HERE, OUTPUT, PRODUCT, STAGE};

mod docs;

pub use docs::{Doc, Word, DOCS, GUIDE};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WordUse {
    pub line: usize,
    pub columns: Range<usize>,
    pub word: Word,
}

pub fn builtin_words(text: &str) -> Vec<WordUse> {
    let bom = usize::from(text.starts_with('\u{feff}'));
    let mut uses = Vec::new();
    let mut found = Vec::new();
    let layout = Layout::new(without_bom(text));
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
                            Json::String(format!("{GUIDE}{}", doc.section).into()),
                        ),
                    ]),
                )
            }),
    );
    [("words", occurrences), ("word_docs", docs)]
}

fn keyword_word(keyword: Keyword) -> Option<Word> {
    Some(match keyword {
        Keyword::Use => Word::Use,
        Keyword::Root => Word::Root,
        Keyword::OutputRoot => Word::OutputRoot,
        Keyword::Source => Word::Source,
        Keyword::Operation => Word::Operation,
        Keyword::Command => Word::Command,
        Keyword::Calculate => Word::Calculate,
        Keyword::Verify => Word::Verify,
        Keyword::Resources => Word::Resources,
        Keyword::Tool => Word::Tool,
        Keyword::Uses => Word::Uses,
        Keyword::Anchored => Word::Anchored,
        Keyword::Tools => Word::ToolsFrom,
        Keyword::Check | Keyword::Checks => Word::Check,
        Keyword::Screen | Keyword::Screens => Word::Screen,
        Keyword::Review | Keyword::Reviews => Word::Review,
        Keyword::Require => Word::Require,
        Keyword::Exclude => Word::Exclude,
        Keyword::Prefer => Word::Prefer,
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
    [
        (Word::Pipeline, "pipeline "),
        (Word::Root, "root "),
        (Word::OutputRoot, "output "),
    ]
    .into_iter()
    .find(|(_, prefix)| line.code.starts_with(prefix) && !line.code.contains('='))
    .map(|(word, prefix)| (word, prefix.len() - 1))
}

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
            Header::Verdicts => (Word::Verdicts, "verdicts".len()),
        };
        found.push((indent..indent + length, word));
        return;
    }
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
        let whole = start == 0 || !matches!(bytes[start - 1], b'.' | b'-' | b':' | b'/');
        if whole {
            if let Some(word) = rule_word(code, start..end, kind) {
                found.push((start..end, word));
            }
        }
        start = end;
    }
}

fn rule_word(code: &str, range: Range<usize>, statement: Option<Word>) -> Option<Word> {
    let text = &code[range.clone()];
    let before = code[..range.start].trim_end();
    let tight = code[..range.start].ends_with('@');
    let spaced = code[range.end..].starts_with(' ');
    let after = code[range.end..].trim_start();
    let selector = match text {
        "vary" => Some(Word::Vary),
        "first" => Some(Word::First),
        "last" => Some(Word::Last),
        "each" => Some(Word::Each),
        "where" => Some(Word::Where),
        "same" => Some(Word::Same),
        "min" => Some(Word::Min),
        "check" => Some(Word::CheckClause),
        "screen" => Some(Word::ScreenClause),
        "review" => Some(Word::ReviewClause),
        _ => None,
    };
    if let Some(word) = selector {
        if tight && after.starts_with('(') {
            return Some(word);
        }
    }
    match (statement?, text) {
        (Word::Operation, "many") if before.ends_with(':') => Some(Word::Many),
        (Word::Operation, "view") if before.contains("->") && before.ends_with('/') => {
            Some(Word::DirectoryView)
        }
        (Word::Operation, "from") if before.contains("->") && before.ends_with('/') => {
            Some(Word::SeedFrom)
        }
        (Word::Operation, "beside") if before.contains("->") => Some(Word::Beside),
        (Word::Source, "beside") if before.split_whitespace().count() > 1 && spaced => {
            Some(Word::Beside)
        }
        (Word::Source, "here")
            if before.split_whitespace().count() > 1
                && (after.is_empty() || after.starts_with('@')) =>
        {
            Some(Word::HereSource)
        }
        (Word::Use, "as") if spaced => Some(Word::UseAs),
        (Word::Use, "from") if spaced => Some(Word::UseFrom),
        (Word::Review, "from") if spaced && before.split_whitespace().count() == 2 => {
            Some(Word::ReviewFrom)
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

fn placeholders(code: &str, statement: Option<Word>, found: &mut Vec<(Range<usize>, Word)>) {
    let in_command = matches!(statement, Some(Word::Command | Word::Verify));
    let in_check = matches!(statement, Some(Word::Check | Word::Screen));
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
            PRODUCT => Some(Word::Product),
            ENTITIES => Some(Word::Entities),
            STAGE => Some(Word::StageName),
            OUTPUT if in_command => Some(Word::Output),
            CORES if in_command => Some(Word::Cores),
            HERE if in_command => Some(Word::Here),
            CHECKED_PATH if in_check => Some(Word::PathPlaceholder),
            _ => None,
        };
        if let Some(word) = word {
            found.push((start..end, word));
        } else if !in_command && !in_check {
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
            for (suffix, word) in [
                (".dir", Word::Dir),
                (".stem", Word::Stem),
                (".prefix", Word::Prefix),
            ] {
                if name.len() > suffix.len() && name.ends_with(suffix) {
                    found.push((end - suffix.len()..end, word));
                }
            }
        }
        index = end + 1;
    }
}

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
