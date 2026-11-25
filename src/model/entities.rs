use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use rustc_hash::{FxHashMap, FxHasher};

#[derive(Clone, Copy)]
struct Symbol {
    id: u32,
    text: &'static str,
}

impl PartialEq for Symbol {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Symbol {}

impl Symbol {
    fn new(text: &str) -> Self {
        static SYMBOLS: OnceLock<Mutex<FxHashMap<&'static str, u32>>> = OnceLock::new();
        let mut symbols = SYMBOLS
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((&text, &id)) = symbols.get_key_value(text) {
            return Self { id, text };
        }
        let id = u32::try_from(symbols.len()).expect("fewer than 2^32 distinct names and values");
        let text: &'static str = Box::leak(text.into());
        symbols.insert(text, id);
        Self { id, text }
    }

    fn cmp_text(self, other: Self) -> Ordering {
        if self == other {
            Ordering::Equal
        } else {
            self.text.cmp(other.text)
        }
    }
}

#[derive(Clone)]
pub struct EntityBinding(Arc<Entities>);

struct Entities {
    pairs: Box<[(Symbol, Symbol)]>,
    hash: u64,
}

impl EntityBinding {
    fn from_sorted(pairs: Vec<(Symbol, Symbol)>) -> Self {
        let mut hasher = FxHasher::default();
        hasher.write_usize(pairs.len());
        for (dimension, value) in &pairs {
            dimension.text.hash(&mut hasher);
            value.text.hash(&mut hasher);
        }
        Self(Arc::new(Entities {
            pairs: pairs.into(),
            hash: hasher.finish(),
        }))
    }

    fn pairs(&self) -> &[(Symbol, Symbol)] {
        &self.0.pairs
    }

    fn pair(&self, dimension: &str) -> Option<Symbol> {
        self.pairs()
            .iter()
            .find(|(name, _)| name.text == dimension)
            .map(|&(_, value)| value)
    }
}

impl Default for EntityBinding {
    fn default() -> Self {
        Self::from_sorted(Vec::new())
    }
}

impl PartialEq for EntityBinding {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.hash == other.0.hash && self.pairs() == other.pairs())
    }
}

impl Eq for EntityBinding {}

impl PartialOrd for EntityBinding {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EntityBinding {
    fn cmp(&self, other: &Self) -> Ordering {
        if Arc::ptr_eq(&self.0, &other.0) {
            return Ordering::Equal;
        }
        self.pairs()
            .iter()
            .zip(other.pairs().iter())
            .map(|(&(left_name, left), &(right_name, right))| {
                left_name
                    .cmp_text(right_name)
                    .then_with(|| left.cmp_text(right))
            })
            .find(|ordering| ordering.is_ne())
            .unwrap_or_else(|| self.pairs().len().cmp(&other.pairs().len()))
    }
}

impl Hash for EntityBinding {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.0.hash);
    }
}

impl fmt::Debug for EntityBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("EntityBinding")
            .field(&DebugPairs(self))
            .finish()
    }
}

struct DebugPairs<'a>(&'a EntityBinding);

impl fmt::Debug for DebugPairs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.iter()).finish()
    }
}

impl From<BTreeMap<String, String>> for EntityBinding {
    fn from(values: BTreeMap<String, String>) -> Self {
        Self::from_sorted(
            values
                .iter()
                .map(|(dimension, value)| (Symbol::new(dimension), Symbol::new(value)))
                .collect(),
        )
    }
}

impl FromIterator<(String, String)> for EntityBinding {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(values: I) -> Self {
        values.into_iter().collect::<BTreeMap<_, _>>().into()
    }
}

impl EntityBinding {
    pub fn get(&self, dimension: &str) -> Option<&str> {
        self.pair(dimension).map(|value| value.text)
    }

    pub fn binds(&self, dimension: &str) -> bool {
        self.pair(dimension).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pairs()
            .iter()
            .map(|(dimension, value)| (dimension.text, value.text))
    }

    pub fn dimensions(&self) -> impl Iterator<Item = &str> {
        self.pairs().iter().map(|(dimension, _)| dimension.text)
    }

    pub fn len(&self) -> usize {
        self.pairs().len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs().is_empty()
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        if other.is_empty() {
            return;
        }
        let (mut mine, mut theirs) = (
            self.pairs().iter().peekable(),
            other.pairs().iter().peekable(),
        );
        let mut pairs = Vec::with_capacity(self.len() + other.len());
        loop {
            let next = match (mine.peek(), theirs.peek()) {
                (None, None) => break,
                (Some(_), None) => mine.next(),
                (None, Some(_)) => theirs.next(),
                (Some(&&(left, _)), Some(&&(right, _))) => match left.cmp_text(right) {
                    Ordering::Less => mine.next(),
                    Ordering::Greater => theirs.next(),
                    Ordering::Equal => {
                        mine.next();
                        theirs.next()
                    }
                },
            };
            pairs.extend(next.copied());
        }
        *self = Self::from_sorted(pairs);
    }

    pub fn from_pairs<const N: usize>(pairs: [(&str, &str); N]) -> Self {
        pairs
            .into_iter()
            .map(|(dimension, value)| (dimension.to_owned(), value.to_owned()))
            .collect()
    }

    pub fn without(&self, dimension: &str) -> Self {
        Self::from_sorted(
            self.pairs()
                .iter()
                .filter(|(name, _)| name.text != dimension)
                .copied()
                .collect(),
        )
    }

    pub fn matches_shared(&self, other: &Self) -> bool {
        self.pairs().iter().all(|&(dimension, value)| {
            other
                .pairs()
                .iter()
                .find(|&&(name, _)| name == dimension)
                .is_none_or(|&(_, other)| other == value)
        })
    }

    pub fn project(&self, dimensions: &[String]) -> Option<Self> {
        if self.len() == dimensions.len()
            && self
                .dimensions()
                .all(|name| dimensions.iter().any(|dimension| dimension == name))
        {
            return Some(self.clone());
        }
        let mut pairs = dimensions
            .iter()
            .map(|dimension| {
                self.pairs()
                    .iter()
                    .find(|(name, _)| name.text == dimension)
                    .copied()
            })
            .collect::<Option<Vec<_>>>()?;
        pairs.sort_unstable_by(|(left, _), (right, _)| left.cmp_text(*right));
        pairs.dedup_by(|(left, _), (right, _)| left == right);
        Some(Self::from_sorted(pairs))
    }

    pub(crate) fn group_key(&self, dimensions: &[String]) -> Option<GroupKey> {
        dimensions
            .iter()
            .map(|dimension| self.pair(dimension).map(|value| value.id))
            .collect::<Option<_>>()
            .map(GroupKey)
    }

    pub(crate) fn within(&self, other: &Self, dimensions: &[String]) -> bool {
        self.pairs().iter().all(|&(dimension, value)| {
            dimensions.iter().any(|name| name == dimension.text)
                && other.pair(dimension.text) == Some(value)
        })
    }

    pub fn cmp_in(&self, other: &Self, dimensions: &[String]) -> Ordering {
        dimensions
            .iter()
            .map(
                |dimension| match (self.pair(dimension), other.pair(dimension)) {
                    (Some(left), Some(right)) if left == right => Ordering::Equal,
                    (Some(left), Some(right)) => natural_cmp(left.text, right.text),
                    (left, right) => left.is_some().cmp(&right.is_some()),
                },
            )
            .find(|ordering| ordering.is_ne())
            .unwrap_or_else(|| self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct GroupKey(Vec<u32>);

pub(crate) fn natural_cmp(left: &str, right: &str) -> Ordering {
    let (mut left_rest, mut right_rest) = (left, right);
    loop {
        match (left_rest.chars().next(), right_rest.chars().next()) {
            (None, None) => return left.cmp(right),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let (a_digits, a_tail) = split_digits(left_rest);
                let (b_digits, b_tail) = split_digits(right_rest);
                let a_number = a_digits.trim_start_matches('0');
                let b_number = b_digits.trim_start_matches('0');
                let ordering = a_number
                    .len()
                    .cmp(&b_number.len())
                    .then_with(|| a_number.cmp(b_number));
                if ordering.is_ne() {
                    return ordering;
                }
                (left_rest, right_rest) = (a_tail, b_tail);
            }
            (Some(a), Some(b)) => {
                if a != b {
                    return a.cmp(&b);
                }
                left_rest = &left_rest[a.len_utf8()..];
                right_rest = &right_rest[b.len_utf8()..];
            }
        }
    }
}

pub(crate) fn near_reason(found: &str, wanted: &str) -> Option<&'static str> {
    if found == wanted {
        return None;
    }
    if found.eq_ignore_ascii_case(wanted) {
        return Some("letter case");
    }
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if digits(found)
        && digits(wanted)
        && found.trim_start_matches('0') == wanted.trim_start_matches('0')
    {
        return Some("leading zeros");
    }
    None
}

fn split_digits(text: &str) -> (&str, &str) {
    let end = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    text.split_at(end)
}

impl fmt::Display for EntityBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (dimension, value)) in self.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "{dimension}={value}")?;
        }
        Ok(())
    }
}

pub(crate) fn identity<'a>(
    product: &str,
    entities: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> String {
    let mut text = String::with_capacity(product.len() + 32);
    push_identity(&mut text, product, entities);
    text
}

pub(crate) fn push_identity<'a>(
    text: &mut String,
    product: &str,
    entities: impl IntoIterator<Item = (&'a str, &'a str)>,
) {
    text.push_str(product);
    let mut entities = entities.into_iter().peekable();
    if entities.peek().is_none() {
        return;
    }
    text.push('[');
    for (index, (dimension, value)) in entities.enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(dimension);
        text.push('=');
        text.push_str(value);
    }
    text.push(']');
}
