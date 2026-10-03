//! A file's path as SPIT reads it for a suggestion: its folders and name
//! as words and the text between them, and its extension.

/// A file's path as its folders and name, each as words and the text
/// around them, with its extension apart.
pub(super) struct Shape<'a> {
    pub(super) file: &'a str,
    /// Each folder, then the name without its extension.
    pub(super) components: Vec<Component<'a>>,
    /// The name's text from its first `.`, as `.nii.gz`.
    pub(super) extension: &'a str,
    /// The word after the last `_` of a BIDS file name, as `bold` in
    /// `sub-01_task-rest_bold`, which tells sources of one shape apart.
    pub(super) suffix: Option<&'a str>,
}

/// A folder or name as words, each a run of letters and digits, and the
/// text between. A word of two or more letters then digits, as `wave3` or
/// `rev2`, is two words with nothing between, the letters naming the
/// digits; `s01` stays whole, as a value.
pub(super) struct Component<'a> {
    pub(super) text: &'a str,
    pub(super) words: Vec<&'a str>,
    /// The text before each word, then the text after the last;
    /// `separators[i]` comes before `words[i]`.
    pub(super) separators: Vec<&'a str>,
}

impl<'a> Component<'a> {
    pub(super) fn of(text: &'a str) -> Self {
        let mut words = Vec::new();
        let mut separators = Vec::new();
        let mut start = 0;
        while let Some(begin) = text[start..].find(|c: char| c.is_ascii_alphanumeric()) {
            let begin = start + begin;
            let end = text[begin..]
                .find(|c: char| !c.is_ascii_alphanumeric())
                .map_or(text.len(), |length| begin + length);
            separators.push(&text[start..begin]);
            let word = &text[begin..end];
            let digits = word.find(|c: char| c.is_ascii_digit()).unwrap_or(0);
            if digits >= 2
                && word[..digits].chars().all(|c| c.is_ascii_alphabetic())
                && word[digits..].chars().all(|c| c.is_ascii_digit())
            {
                words.push(&word[..digits]);
                separators.push("");
                words.push(&word[digits..]);
            } else {
                words.push(word);
            }
            start = end;
        }
        separators.push(&text[start..]);
        Self {
            text,
            words,
            separators,
        }
    }
}

impl<'a> Shape<'a> {
    pub(super) fn of(file: &'a str) -> Self {
        let name_start = file.rfind('/').map_or(0, |slash| slash + 1);
        let body_end = file[name_start..]
            .find('.')
            .filter(|&dot| dot > 0)
            .map_or(file.len(), |dot| name_start + dot);
        Self {
            file,
            components: file[..body_end].split('/').map(Component::of).collect(),
            extension: &file[body_end..],
            suffix: bids_suffix(&file[name_start..body_end]),
        }
    }

    /// What files of one source share: how deep they are, their extension
    /// and BIDS suffix, and their top folder when it is a plain word, as
    /// `baseline` or `readings`, since such folders usually hold different
    /// kinds of data.
    pub(super) fn key(&self) -> (usize, Option<&'a str>, &'a str, Option<&'a str>) {
        let top = self.components[0].text;
        let plain =
            self.components.len() > 1 && top.chars().all(|c| c.is_ascii_alphabetic() || c == '_');
        (
            self.components.len(),
            plain.then_some(top),
            self.extension,
            self.suffix,
        )
    }

    /// The text between the words of every component, which files of one
    /// shape share.
    pub(super) fn separators(&self) -> Vec<&'a str> {
        let mut all = Vec::new();
        for component in &self.components {
            all.extend(&component.separators);
            all.push("/");
        }
        all
    }
}

/// The suffix of a BIDS-style name, `bold` in `sub-01_task-rest_bold`: the
/// last of its `_` parts, when an earlier part is a `key-value` entity.
fn bids_suffix(name: &str) -> Option<&str> {
    let (entities, suffix) = name.rsplit_once('_')?;
    let entity = |part: &str| {
        part.split_once('-').is_some_and(|(key, value)| {
            !key.is_empty()
                && key.chars().all(|c| c.is_ascii_alphabetic())
                && !value.is_empty()
                && value.chars().all(|c| c.is_ascii_alphanumeric())
        })
    };
    let is_word = !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_alphanumeric());
    (is_word && entities.split('_').any(entity)).then_some(suffix)
}
