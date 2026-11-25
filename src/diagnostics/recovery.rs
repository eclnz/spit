use std::collections::BTreeSet;

use rustc_hash::FxHashSet;

use crate::parser::{parse_use, Keyword, Layout, Line, UseSpec};
use crate::{ParseError, ParseErrorKind};

pub(super) fn recover_parse_errors<T>(
    text: &str,
    parse: impl FnOnce(&str) -> (T, Vec<ParseError>),
) -> Result<T, Vec<ParseError>> {
    let (parsed, errors) = recover_document(text, parse);
    if errors.is_empty() {
        Ok(parsed)
    } else {
        Err(errors)
    }
}

pub(crate) fn recover_document<T>(
    text: &str,
    parse: impl FnOnce(&str) -> (T, Vec<ParseError>),
) -> (T, Vec<ParseError>) {
    let (parsed, mut found) = parse(text);
    if found.is_empty() {
        return (parsed, found);
    }
    found.sort_by_key(ParseError::line);
    let layout = Layout::new(text);
    let mut failures = Failures::new(&layout.lines, found.iter().map(ParseError::line).collect());
    let mut errors = Vec::new();
    for error in found {
        if !failures.explains(&error) && !failures.empties_body(&error) {
            failures.note(&error);
            errors.push(error);
        }
    }
    (parsed, errors)
}

struct Failures<'a> {
    lines: &'a [Line<'a>],
    above: Vec<Option<usize>>,
    named: FxHashSet<String>,
    unnamed: Vec<&'a str>,
    failed_uses: Vec<UseSpec>,
    error_lines: BTreeSet<usize>,
}

impl<'a> Failures<'a> {
    fn new(lines: &'a [Line<'a>], error_lines: BTreeSet<usize>) -> Self {
        let mut above = Vec::with_capacity(lines.len());
        let mut header = None;
        for (index, line) in lines.iter().enumerate() {
            above.push(header);
            if !line.text.trim().is_empty() && !body_line(line.text) {
                header = Some(index);
            }
        }
        Self {
            lines,
            above,
            named: FxHashSet::default(),
            unnamed: Vec::new(),
            failed_uses: Vec::new(),
            error_lines,
        }
    }

    fn note(&mut self, error: &ParseError) {
        let failed = error.line().saturating_sub(1);
        let Some(line) = self.lines.get(failed) else {
            return;
        };
        self.declare(line);
        if line.text.starts_with(char::is_whitespace) {
            if let Some(header) = self.above[failed] {
                self.declare(&self.lines[header]);
            }
        }
    }

    fn declare(&mut self, line: &Line<'a>) {
        if line.keyword() == Some(Keyword::Use) {
            if let Ok(spec) = parse_use(line.code, 0) {
                self.failed_uses.push(spec);
            }
            return;
        }
        let Some(declaration) = line.text.trim().strip_prefix("operation ") else {
            return;
        };
        match declaration.split_once('(') {
            Some((name, _)) => {
                self.named.insert(name.trim().to_owned());
            }
            None => self.unnamed.push(declaration),
        }
    }

    fn explains(&self, error: &ParseError) -> bool {
        if error.location.imported().is_some() {
            return false;
        }
        let ParseErrorKind::UndeclaredOperation { name: operation } = error.kind() else {
            return false;
        };
        self.named.contains(operation.as_str())
            || self.failed_uses.iter().any(|spec| {
                let local = match &spec.alias {
                    Some(alias) => operation
                        .strip_prefix(alias.as_str())
                        .and_then(|rest| rest.strip_prefix("::")),
                    None => Some(operation.as_str()),
                };
                local.is_some_and(|local| {
                    spec.names
                        .as_ref()
                        .is_none_or(|names| names.iter().any(|name| name == local))
                })
            })
            || self
                .unnamed
                .iter()
                .any(|declaration| declaration.starts_with(operation.as_str()))
    }

    fn empties_body(&self, error: &ParseError) -> bool {
        if error.location.imported().is_some()
            || !matches!(error.kind(), ParseErrorKind::EmptyBody { .. })
        {
            return false;
        }
        let header = error.line();
        let end = self
            .lines
            .iter()
            .enumerate()
            .skip(header)
            .find(|(_, line)| {
                !line.text.trim().is_empty() && !line.text.starts_with(char::is_whitespace)
            })
            .map_or(self.lines.len(), |(index, _)| index);
        header < end && self.error_lines.range(header + 1..=end).next().is_some()
    }
}

fn body_line(line: &str) -> bool {
    line.starts_with(char::is_whitespace) && !line.trim_start().starts_with("operation ")
}
