//! Where an error in an imported file is, and the paths messages give it by.

use std::path::{Component, Path};
use std::sync::Arc;

use crate::parser::{without_bom, ParseError};
use crate::span::{content_columns, Imported, Place, UseLine};

/// `file`, a path from the folder of the file `import` names, as a path
/// from the folder of the file that imports it; `import` itself for `""`.
pub(super) fn rebase(import: &str, file: &str) -> String {
    let import = import.replace('\\', "/");
    let folder = import.rsplit_once('/').map_or("", |(folder, _)| folder);
    let joined = Path::new(folder).join(file);
    let path = if file.is_empty() {
        Path::new(&import)
    } else {
        &joined
    };
    relative_path(Path::new(""), path)
}

/// Where a `use` line that reads a file with an error is.
pub(super) struct ImportedAt<'a> {
    pub(super) number: usize,
    pub(super) use_line: &'a str,
    /// The path the line gives, from its own file's folder.
    pub(super) spec_path: &'a str,
    /// The imported file's text.
    pub(super) text: &'a str,
}

/// `error`, found in the file a `use` line reads, as an error of the file
/// that has the line: reported at the line, with where it is in the imported
/// file. `rebase` gives a path from that file's folder.
pub(super) fn in_import(
    error: &ParseError,
    at: ImportedAt<'_>,
    rebase: impl Fn(&str) -> String,
) -> ParseError {
    let mut imported = match error.location.imported() {
        // Found further in: its files are from the imported file's folder.
        Some(inner) => Imported {
            file: rebase(&inner.file),
            place: inner.place.clone(),
            text: Arc::clone(&inner.text),
            uses: inner
                .uses
                .iter()
                .map(|used| UseLine {
                    file: Some(
                        used.file
                            .as_deref()
                            .map_or_else(|| at.spec_path.to_owned(), &rebase),
                    ),
                    ..used.clone()
                })
                .collect(),
        },
        None => {
            let text = without_bom(at.text);
            let place = error.location.place_in(text).or_else(|| {
                let line = error.location.line?;
                let content = text.lines().nth(line.checked_sub(1)?)?;
                Some(Place::new(line, content_columns(content)))
            });
            Imported {
                file: at.spec_path.to_owned(),
                place: place.unwrap_or_else(|| Place::new(error.line(), 0..0)),
                text: Arc::from(text),
                uses: Vec::new(),
            }
        }
    };
    imported.uses.push(UseLine {
        file: None,
        place: Place::new(at.number, content_columns(at.use_line)),
        text: at.use_line.to_owned(),
    });
    let mut wrapped = ParseError::new(at.number, error.message());
    wrapped.location.set_imported(imported);
    wrapped
}

/// `error` with the file it is in named: the `use` lines of the file being
/// read, which an import left unnamed, are in `file`.
pub(super) fn named_in(mut error: ParseError, file: &str) -> ParseError {
    if let Some(imported) = error.location.imported_mut() {
        for used in &mut imported.uses {
            used.file.get_or_insert_with(|| file.to_owned());
        }
    }
    error
}

/// `target` as a path from the folder `base`, written with `/` and `..`, so
/// that a message names a file the same wherever the checkout is. Both are
/// absolute, or both are not.
pub(super) fn relative_path(base: &Path, target: &Path) -> String {
    fn normal(path: &Path) -> Vec<Component<'_>> {
        let mut parts: Vec<Component<'_>> = Vec::new();
        for part in path.components() {
            match part {
                Component::CurDir => {}
                Component::ParentDir if matches!(parts.last(), Some(Component::Normal(_))) => {
                    parts.pop();
                }
                part => parts.push(part),
            }
        }
        parts
    }
    if base.is_absolute() != target.is_absolute() {
        return target.display().to_string();
    }
    let (base, target) = (normal(base), normal(target));
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let parts: Vec<String> = base[common..]
        .iter()
        .map(|_| "..".to_owned())
        .chain(
            target[common..]
                .iter()
                .map(|part| part.as_os_str().to_string_lossy().into_owned()),
        )
        .collect();
    parts.join("/")
}
