//! Path templates: a rule's text read into parts, completed for a product,
//! and written back. Every step shares this: it knows the model, and nothing
//! about resolving or discovery.

use std::borrow::Cow;
use std::fmt;

use crate::span::Located;
use crate::template::{parse_template, Part};

use super::components::encode_component;

crate::span::message_error!(
    /// What is wrong with a path rule, or with the paths it gives artifacts.
    PathProblem
);

/// An error in a path rule, or about the paths it gives artifacts.
pub type PathError = Located<PathProblem>;

/// What a `{name}` in a path template stands for.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum PathPlaceholder {
    /// `{@product}`: the product's name.
    Product,
    /// `{@entities}`: every dimension as `dim=value`, in declared order.
    Entities,
    /// `{@stage}`: the stage whose block holds the step.
    Stage,
    /// `{@labels}`: every dimension as `dim-value`, joined by `_`, as BIDS
    /// names them. A product's template has it written out.
    Labels,
    /// Any other name: a dimension the product declares.
    Dimension(String),
}

impl PathPlaceholder {
    /// The built-in placeholder `{@name}` writes, if any.
    fn built_in(name: &str) -> Option<Self> {
        match name {
            "product" => Some(Self::Product),
            "entities" => Some(Self::Entities),
            "stage" => Some(Self::Stage),
            "labels" => Some(Self::Labels),
            _ => None,
        }
    }

    /// The placeholder `{name}` is: a built-in as `@product`, or else a
    /// dimension, which takes no `@`.
    fn parse(name: String) -> Result<Self, String> {
        match name.strip_prefix('@') {
            Some(built_in) => Self::built_in(built_in).ok_or_else(|| {
                format!(
                    "unknown built-in placeholder `{{{name}}}`; path templates have `{{@product}}`, `{{@entities}}`, `{{@labels}}` and `{{@stage}}`"
                )
            }),
            None => Ok(Self::Dimension(name)),
        }
    }

    /// What to write instead when a product has no dimension `name`, if
    /// `name` is a built-in written without its `@`.
    pub(crate) fn hint(name: &str) -> Option<String> {
        let meaning = match Self::built_in(name)? {
            Self::Product => "the product's name",
            Self::Entities => "every dimension as `dimension=value`",
            Self::Stage => "the stage that makes it",
            Self::Labels => "every dimension as `dimension-value`",
            Self::Dimension(_) => return None,
        };
        Some(format!("; write `{{@{name}}}` for {meaning}"))
    }

    /// The name between the braces, with a built-in's `@`.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Product => "@product",
            Self::Entities => "@entities",
            Self::Stage => "@stage",
            Self::Labels => "@labels",
            Self::Dimension(name) => name,
        }
    }
}

/// Reads as the placeholder is written, such as `{@stage}`.
impl fmt::Display for PathPlaceholder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{}}}", self.name())
    }
}

/// A path template's literal text and placeholders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PathPart {
    Literal(String),
    Placeholder(PathPlaceholder),
    /// `[...]`: text and placeholders a product's path keeps only when each
    /// placeholder in it has a value for that product. A product's template
    /// has its groups resolved.
    Group(Vec<PathPart>),
}

/// What a product gives the placeholders of a path template: its
/// dimensions, in the pipeline's order, and whether a stage makes it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Holder<'a> {
    pub(crate) dimensions: &'a [String],
    pub(crate) in_stage: bool,
}

impl Holder<'_> {
    /// Whether `placeholder` has a value for the product.
    fn has(self, placeholder: &PathPlaceholder) -> bool {
        match placeholder {
            PathPlaceholder::Product | PathPlaceholder::Entities => true,
            PathPlaceholder::Stage => self.in_stage,
            PathPlaceholder::Labels => !self.dimensions.is_empty(),
            PathPlaceholder::Dimension(name) => self.dimensions.contains(name),
        }
    }

    /// Whether the product keeps `group`: each placeholder in it has a value.
    fn keeps(self, group: &[PathPart]) -> bool {
        group.iter().all(|part| match part {
            PathPart::Placeholder(placeholder) => self.has(placeholder),
            _ => true,
        })
    }
}

/// A path rule's template, parsed once when the rule is read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathTemplate {
    pub(crate) text: String,
    parts: Vec<PathPart>,
}

impl PathTemplate {
    /// Where outputs go when a pipeline run from a recipe sets no `path:`.
    pub fn default_output() -> Self {
        Self::parse("out/{@product}/{@entities}").expect("built-in output path is valid")
    }

    /// Parse a template such as `derivatives/{@stage}/{@product}/{@entities}.mif`.
    pub fn parse(text: impl Into<String>) -> Result<Self, PathError> {
        let text = text.into();
        let parts = parse_parts(&text).map_err(error)?;
        Ok(Self { text, parts })
    }

    /// Whether the template has a `[...]` group or `{@labels}`, which each
    /// product resolves its own way.
    pub(crate) fn varies(&self) -> bool {
        self.parts.iter().any(|part| {
            matches!(
                part,
                PathPart::Group(_) | PathPart::Placeholder(PathPlaceholder::Labels)
            )
        })
    }

    /// This template as `holder`'s product has it: each group kept whole or
    /// dropped, and `{@labels}` written out as `sub-{sub}_ses-{ses}`. One
    /// with no group or `{@labels}` is the same for every product.
    pub(crate) fn resolve(&self, holder: Holder<'_>) -> Cow<'_, Self> {
        if !self.varies() {
            return Cow::Borrowed(self);
        }
        let mut parts = Vec::new();
        for part in &self.parts {
            match part {
                PathPart::Group(group) if holder.keeps(group) => {
                    for part in group {
                        push_resolved(&mut parts, part, holder);
                    }
                }
                PathPart::Group(_) => {}
                part => push_resolved(&mut parts, part, holder),
            }
        }
        if parts.is_empty() {
            parts.push(PathPart::Literal(String::new()));
        }
        Cow::Owned(Self {
            text: render(&parts),
            parts,
        })
    }

    /// Whether `holder`'s product's path has `{@labels}` written out in it.
    pub(crate) fn writes_labels(&self, holder: Holder<'_>) -> bool {
        let labels = |part: &PathPart| *part == PathPart::Placeholder(PathPlaceholder::Labels);
        holder.has(&PathPlaceholder::Labels)
            && self.parts.iter().any(|part| match part {
                PathPart::Group(group) => holder.keeps(group) && group.iter().any(labels),
                part => labels(part),
            })
    }

    /// Each dimension a `[...]` group names.
    pub(crate) fn group_dimensions(&self) -> impl Iterator<Item = &str> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                PathPart::Group(group) => Some(group),
                _ => None,
            })
            .flatten()
            .filter_map(|part| match part {
                PathPart::Placeholder(PathPlaceholder::Dimension(name)) => Some(name.as_str()),
                _ => None,
            })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn parts(&self) -> &[PathPart] {
        &self.parts
    }

    /// The extension the template ends with: the text of its last file
    /// name after its final placeholder, from the first `.`, as `.nii.gz`
    /// in `sub-{sub}_T1w.nii.gz`. Values cannot add a `.`, as they are
    /// escaped, so only the template's own text can.
    pub fn extension(&self) -> Option<&str> {
        let Some(PathPart::Literal(tail)) = self.parts.last() else {
            return None;
        };
        let name = tail.rsplit('/').next().unwrap_or(tail);
        name.find('.').map(|dot| &name[dot..])
    }

    /// Whether the template ends with `extension` after its last placeholder,
    /// as `sub-{sub}_acq-1.5T.nii.gz` ends with `.nii.gz`, though the
    /// extension [`PathTemplate::extension`] reads in it is `.5T.nii.gz`.
    pub(crate) fn ends_with(&self, extension: &str) -> bool {
        matches!(self.parts.last(), Some(PathPart::Literal(tail)) if tail.ends_with(extension))
    }

    /// This template without `extension` at its end, or `None` when it does
    /// not end with it.
    #[must_use]
    pub(crate) fn without_extension(&self, extension: &str) -> Option<Self> {
        let mut parts = self.parts.clone();
        let Some(PathPart::Literal(tail)) = parts.last_mut() else {
            return None;
        };
        tail.truncate(tail.strip_suffix(extension)?.len());
        if tail.is_empty() {
            parts.pop();
        }
        Some(Self {
            text: self.text.strip_suffix(extension)?.to_owned(),
            parts,
        })
    }

    /// This template with `extension` added to its end.
    #[must_use]
    pub(crate) fn with_extension(&self, extension: &str) -> Self {
        let mut parts = self.parts.clone();
        match parts.last_mut() {
            Some(PathPart::Literal(tail)) => tail.push_str(extension),
            _ => parts.push(PathPart::Literal(extension.to_owned())),
        }
        Self {
            text: format!("{}{extension}", self.text),
            parts,
        }
    }

    /// This template with `{@product}` written out as `name`, so that an
    /// imported product keeps the path its own file gives it.
    #[must_use]
    pub(crate) fn with_product(&self, name: &str) -> Self {
        fn named(parts: &[PathPart], name: &str) -> Vec<PathPart> {
            parts
                .iter()
                .map(|part| match part {
                    PathPart::Placeholder(PathPlaceholder::Product) => {
                        PathPart::Literal(name.to_owned())
                    }
                    PathPart::Group(group) => PathPart::Group(named(group, name)),
                    part => part.clone(),
                })
                .collect()
        }
        let parts = named(&self.parts, name);
        Self {
            text: render(&parts),
            parts,
        }
    }
}

impl fmt::Display for PathTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq<str> for PathTemplate {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl PartialEq<&str> for PathTemplate {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

/// Read a path template's parts: `[...]` groups, each holding text and
/// placeholders, between text and placeholders. `[[` and `]]` are literal
/// brackets, as `{{` and `}}` are literal braces.
fn parse_parts(text: &str) -> Result<Vec<PathPart>, String> {
    let mut parts = Vec::new();
    // The text since the last bracket, with `[[` and `]]` read.
    let mut segment = String::new();
    // Where the open group's `[` is.
    let mut open = None;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        match character {
            '[' | ']' if characters.peek().map(|&(_, next)| next) == Some(character) => {
                characters.next();
                segment.push(character);
            }
            '[' => {
                if open.is_some() {
                    return Err(format!(
                        "`[` inside `[...]` in `{text}`; a group cannot hold another"
                    ));
                }
                push_segment(&mut parts, &std::mem::take(&mut segment))?;
                open = Some(index);
            }
            ']' => {
                let start = open.take().ok_or_else(|| {
                    format!("unmatched `]` in `{text}`; write `]]` for a literal `]`")
                })?;
                let mut group = Vec::new();
                push_segment(&mut group, &std::mem::take(&mut segment))?;
                if !group.iter().any(|part| {
                    matches!(
                        part,
                        PathPart::Placeholder(
                            PathPlaceholder::Dimension(_)
                                | PathPlaceholder::Stage
                                | PathPlaceholder::Labels
                        )
                    )
                }) {
                    return Err(format!(
                        "`{}` names nothing that could be absent; remove the brackets",
                        &text[start..=index]
                    ));
                }
                parts.push(PathPart::Group(group));
            }
            character => segment.push(character),
        }
    }
    if open.is_some() {
        return Err(format!(
            "unclosed `[` in `{text}`; write `[[` for a literal `[`"
        ));
    }
    push_segment(&mut parts, &segment)?;
    if parts.is_empty() {
        parts.push(PathPart::Literal(String::new()));
    }
    Ok(parts)
}

/// Add the text and placeholders of `segment`, text between brackets.
fn push_segment(parts: &mut Vec<PathPart>, segment: &str) -> Result<(), String> {
    if segment.is_empty() {
        return Ok(());
    }
    for part in parse_template(segment)? {
        match part {
            Part::Literal(value) => push_literal(parts, &value),
            Part::Placeholder(name) => {
                parts.push(PathPart::Placeholder(PathPlaceholder::parse(name)?));
            }
        }
    }
    Ok(())
}

/// Add `value` to the text `parts` ends with, so that text is one part.
fn push_literal(parts: &mut Vec<PathPart>, value: &str) {
    match parts.last_mut() {
        Some(PathPart::Literal(tail)) => tail.push_str(value),
        _ => parts.push(PathPart::Literal(value.to_owned())),
    }
}

/// Add `part` of a group `holder`'s product keeps, or of no group, with
/// `{@labels}` written out when the product has dimensions.
fn push_resolved(parts: &mut Vec<PathPart>, part: &PathPart, holder: Holder<'_>) {
    match part {
        PathPart::Literal(value) => push_literal(parts, value),
        PathPart::Placeholder(PathPlaceholder::Labels) if holder.has(&PathPlaceholder::Labels) => {
            for (index, dimension) in holder.dimensions.iter().enumerate() {
                if index > 0 {
                    push_literal(parts, "_");
                }
                push_literal(parts, &format!("{}-", encode_component(dimension)));
                parts.push(PathPart::Placeholder(PathPlaceholder::Dimension(
                    dimension.clone(),
                )));
            }
        }
        part => parts.push(part.clone()),
    }
}

/// The text of `parts`, as a template would write them.
fn render(parts: &[PathPart]) -> String {
    let mut text = String::new();
    for part in parts {
        match part {
            PathPart::Literal(value) => {
                for character in value.chars() {
                    if matches!(character, '{' | '}' | '[' | ']') {
                        text.push(character);
                    }
                    text.push(character);
                }
            }
            PathPart::Placeholder(placeholder) => text.push_str(&placeholder.to_string()),
            PathPart::Group(group) => {
                text.push('[');
                text.push_str(&render(group));
                text.push(']');
            }
        }
    }
    text
}

pub(crate) fn error(message: impl Into<String>) -> PathError {
    PathError::new(message)
}

/// Fail unless `root`, where a dataset's source files are, is a directory.
pub(crate) fn require_directory(root: &std::path::Path) -> Result<(), PathError> {
    if root.is_dir() {
        return Ok(());
    }
    Err(error(format!(
        "source root is not a directory: `{}`",
        root.display()
    )))
}
