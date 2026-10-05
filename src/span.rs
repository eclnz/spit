//! Column ranges within a line of text, in bytes, for pointing at source,
//! and the location every error in the pipeline text carries.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

/// A line and a byte range within it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Place {
    pub(crate) line: usize,
    pub(crate) columns: Range<usize>,
}

impl Place {
    pub(crate) fn new(line: usize, columns: Range<usize>) -> Self {
        Self { line, columns }
    }
}

/// Where in the pipeline text an error is, as far as it is known.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Location {
    pub line: Option<usize>,
    /// The byte range in that line.
    pub columns: Option<Range<usize>>,
    /// The part of the line the error is about, before its columns are known.
    pub(crate) focus: Option<Focus>,
}

/// An error in an imported file's own text: which file, and where in it,
/// and the `use` lines that read it, nearest the file first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Imported {
    /// The file's path from the folder of the pipeline being checked.
    pub(crate) file: String,
    /// The error's line and columns in that file.
    pub(crate) place: Place,
    /// That file's text, to count columns in characters or UTF-16 code units.
    pub(crate) text: Arc<str>,
    pub(crate) uses: Vec<UseLine>,
}

/// A `use` line that reads an imported file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UseLine {
    /// The file the line is in, from the pipeline's folder; `None` while it
    /// is the file being read, until the pipeline's own name is known.
    pub(crate) file: Option<String>,
    pub(crate) place: Place,
    pub(crate) text: String,
}

/// The part of a line an error is about, before its columns are known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Focus {
    /// Text within the error's columns, or elsewhere on its line, such as
    /// one `{placeholder}`, found when the error is reported.
    Text(Box<str>),
    /// A slice of the line being parsed, by its [`address_of`], resolved to
    /// columns with [`columns_at`] once the parser is back at the line.
    Address(Range<usize>),
    /// The error is in a file the text imports: the line and columns of the
    /// error are those of the `use` line that reads it, and this says where
    /// it is in that file. Kept here, not in a field of its own, so that an
    /// error stays small on the path that succeeds.
    Imported(Box<Imported>),
}

impl Location {
    /// Where the error is in the imported file that has it, when it is in one.
    pub(crate) fn imported(&self) -> Option<&Imported> {
        match &self.focus {
            Some(Focus::Imported(imported)) => Some(imported),
            _ => None,
        }
    }

    pub(crate) fn imported_mut(&mut self) -> Option<&mut Imported> {
        match &mut self.focus {
            Some(Focus::Imported(imported)) => Some(imported),
            _ => None,
        }
    }

    /// Mark the error as in an imported file.
    pub(crate) fn set_imported(&mut self, imported: Imported) {
        self.focus = Some(Focus::Imported(Box::new(imported)));
    }

    /// The line and columns, narrowed to the focus when `text` contains it:
    /// first inside the columns, then as a word elsewhere on the line, such
    /// as the operation a command is declared for.
    pub(crate) fn place_in(&self, text: &str) -> Option<Place> {
        let (line, columns) = (self.line?, self.columns.clone()?);
        let focus = match &self.focus {
            Some(Focus::Text(focus)) => Some(focus),
            _ => None,
        };
        let focus = focus.and_then(|focus| {
            let content = text.lines().nth(line.checked_sub(1)?)?;
            let inside = content
                .get(columns.clone())?
                .find(&**focus)
                .map(|offset| columns.start + offset..columns.start + offset + focus.len());
            inside.or_else(|| find_word(content, content_columns(content).start, focus))
        });
        Some(Place::new(line, focus.unwrap_or(columns)))
    }
}

/// An error and where in the pipeline text it is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Located<E> {
    pub error: E,
    pub location: Location,
}

impl<E> Located<E> {
    /// An error whose location is not known yet.
    pub(crate) fn unplaced(error: E) -> Self {
        Self {
            error,
            location: Location::default(),
        }
    }

    /// The error's message, without its line.
    pub fn message(&self) -> String
    where
        E: fmt::Display,
    {
        self.error.to_string()
    }

    /// Attach a place unless a more specific one is already recorded.
    pub(crate) fn at(mut self, place: Option<Place>) -> Self {
        if self.location.line.is_none() {
            if let Some(place) = place {
                self.location.line = Some(place.line);
                self.location.columns = Some(place.columns);
            }
        }
        self
    }

    /// Mark text on the error's line as what it is about.
    pub(crate) fn focus(mut self, text: impl Into<String>) -> Self {
        self.location.focus = Some(Focus::Text(text.into().into_boxed_str()));
        self
    }
}

impl<E: From<String>> Located<E> {
    /// An error that is only a message, such as one about a command or a
    /// path template, whose location is not known yet.
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self::unplaced(E::from(message.into()))
    }
}

/// Declare an error that is a message about one kind of thing, so that
/// errors about different things are different types.
macro_rules! message_error {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $name(pub String);

        impl From<String> for $name {
            fn from(message: String) -> Self {
                Self(message)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}
pub(crate) use message_error;

/// Reads as `line 3: message`, or just the message without a line. An error
/// in an imported file reads as `line 1: in `libs/lib.spit` at line 4:
/// message`, with the line of the `use` line first.
impl<E: fmt::Display> fmt::Display for Located<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.location.line {
            write!(f, "line {line}: ")?;
        }
        if let Some(imported) = self.location.imported() {
            write!(
                f,
                "in `{}` at line {}: ",
                imported.file, imported.place.line
            )?;
        }
        self.error.fmt(f)
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for Located<E> {}

/// The byte range `part` occupies in `line`, when `part` is a slice of `line`.
///
/// Parsing works on slices of the original line, so this recovers where a
/// token came from without the parser tracking offsets itself.
pub(crate) fn columns_of(line: &str, part: &str) -> Option<Range<usize>> {
    columns_at(line, &address_of(part))
}

/// Where `part` sits in memory, to find it later in the line it was sliced
/// from with [`columns_at`], when the parser has only the slice.
///
/// This and [`columns_at`] are the only places SPIT reads an address; they
/// do what the unstable `str::substr_range` does, and can use it once it is
/// stable.
pub(crate) fn address_of(part: &str) -> Range<usize> {
    let start = part.as_ptr() as usize;
    start..start + part.len()
}

/// The byte range of `line` at `address`, from [`address_of`], when the
/// address lies within `line` on character boundaries. An address from any
/// other text gives `None`, never columns that could split a character.
pub(crate) fn columns_at(line: &str, address: &Range<usize>) -> Option<Range<usize>> {
    let base = line.as_ptr() as usize;
    let start = address.start.checked_sub(base)?;
    let end = address.end.checked_sub(base)?;
    (start <= end && line.get(start..end).is_some()).then_some(start..end)
}

/// The range of `line` without leading indentation, trailing space, or a
/// trailing comment.
pub(crate) fn content_columns(line: &str) -> Range<usize> {
    let code = crate::parser::strip_comment(line);
    let start = code.len() - code.trim_start().len();
    start..code.trim_end().len().max(start)
}

/// The first occurrence of `word` in `line` at or after `from` that is not
/// part of a longer name.
pub(crate) fn find_word(line: &str, from: usize, word: &str) -> Option<Range<usize>> {
    if word.is_empty() {
        return None;
    }
    let is_name = |character: char| character.is_ascii_alphanumeric() || character == '_';
    let mut start = from;
    while let Some(offset) = line.get(start..)?.find(word) {
        let begin = start + offset;
        let end = begin + word.len();
        let before = line[..begin].chars().next_back();
        let after = line[end..].chars().next();
        if !before.is_some_and(is_name) && !after.is_some_and(is_name) {
            return Some(begin..end);
        }
        start = begin + word.len();
    }
    None
}

/// The lines of one text, collected once so that a line is found by its
/// number in one step, not by counting from the top. They are what
/// `str::lines` gives, so a final empty line and a trailing `\r` are as
/// `lines` treats them.
pub(crate) struct Lines<'a>(Vec<&'a str>);

impl<'a> Lines<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self(text.lines().collect())
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.0.iter().copied()
    }

    /// The line numbered `line`, from 1.
    pub(crate) fn get(&self, line: usize) -> Option<&'a str> {
        self.0.get(line.checked_sub(1)?).copied()
    }
}

/// The UTF-16 code unit range of a byte range in `line`, as editors count.
pub(crate) fn utf16_columns(line: &str, columns: &Range<usize>) -> Range<usize> {
    let units = |end: usize| {
        line.get(..end.min(line.len()))
            .map_or(0, |prefix| prefix.encode_utf16().count())
    };
    units(columns.start)..units(columns.end)
}

#[cfg(test)]
mod tests {
    use super::{address_of, columns_at, columns_of, content_columns};

    #[test]
    fn a_slice_is_found_in_the_line_it_came_from() {
        let line = "  source é: Image  # note";
        let token = &line[9..11];
        assert_eq!(columns_of(line, token), Some(9..11));
        assert_eq!(columns_of(line, &line[..0]), Some(0..0));
        assert_eq!(
            columns_of(line, &line[line.len()..]),
            Some(line.len()..line.len())
        );
    }

    #[test]
    fn an_address_outside_the_line_or_inside_a_character_is_rejected() {
        let line = "source é";
        let other = String::from("source é");
        assert_eq!(columns_of(line, &other), None);
        let address = address_of(line);
        let inside = address.start + 8..address.end;
        assert_eq!(columns_at(line, &inside), None);
        let past = address.start..address.end + 1;
        assert_eq!(columns_at(line, &past), None);
    }

    #[test]
    fn content_leaves_out_indentation_trailing_space_and_comments() {
        assert_eq!(content_columns("  a b  # c"), 2..5);
        assert_eq!(content_columns("a"), 0..1);
        assert_eq!(content_columns("   "), 3..3);
        assert_eq!(content_columns("  # only"), 2..2);
        assert_eq!(content_columns(""), 0..0);
    }
}
