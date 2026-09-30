use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use rustc_hash::{FxHashMap, FxHashSet, FxHasher};

use crate::command::CommandTemplate;
use crate::error::{DefinitionSubject, ResolveError};
use crate::paths::PathTemplate;
use crate::types::TypeExpr;

pub type ArtifactType = TypeExpr;

/// A dimension's name or value, interned: each distinct text is kept once
/// for the life of the process and numbered, so symbols compare equal by
/// number. A dataset has few distinct names and values, however many
/// artifacts bind them.
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

    /// Compare as text; equal symbols are equal text.
    fn cmp_text(self, other: Self) -> Ordering {
        if self == other {
            Ordering::Equal
        } else {
            self.text.cmp(other.text)
        }
    }
}

/// The value an artifact has for each of its product's dimensions. Every
/// job an artifact reaches holds a copy of it, so copies share one list of
/// pairs, sorted by dimension name, and its hash is kept with it, so
/// artifacts are cheap to look up. Bindings order by their values alone.
#[derive(Clone)]
pub struct EntityBinding(Arc<Entities>);

/// Behind one pointer, so a binding held by every artifact and job it
/// reaches costs a pointer each.
struct Entities {
    pairs: Box<[(Symbol, Symbol)]>,
    hash: u64,
}

impl EntityBinding {
    /// A binding of `pairs`, which are sorted by dimension name, each once.
    fn from_sorted(pairs: Vec<(Symbol, Symbol)>) -> Self {
        // The hash a `BTreeMap<String, String>` of the same pairs has. Keep
        // in step with `Ord` below: maps keyed by bindings iterate as they
        // did when bindings were maps, which output order depends on.
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
    /// As a `BTreeMap` of the same pairs orders. Keep in step with the hash
    /// in `from_sorted`; output that sorts by binding depends on this order.
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

/// A binding's pairs, shown as a map.
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
    /// The value bound to `dimension`, if any.
    pub fn get(&self, dimension: &str) -> Option<&str> {
        self.pair(dimension).map(|value| value.text)
    }

    /// Whether `dimension` has a value.
    pub fn binds(&self, dimension: &str) -> bool {
        self.pair(dimension).is_some()
    }

    /// Each dimension and its value, in dimension name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pairs()
            .iter()
            .map(|(dimension, value)| (dimension.text, value.text))
    }

    /// The dimensions with a value, in name order.
    pub fn dimensions(&self) -> impl Iterator<Item = &str> {
        self.pairs().iter().map(|(dimension, _)| dimension.text)
    }

    /// How many dimensions have a value.
    pub fn len(&self) -> usize {
        self.pairs().len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs().is_empty()
    }

    /// Add `other`'s values, replacing any this has for the same dimensions.
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

    /// Without `dimensions`' values.
    pub(crate) fn except(&self, dimensions: &[String]) -> Self {
        Self::from_sorted(
            self.pairs()
                .iter()
                .filter(|(name, _)| !dimensions.iter().any(|dimension| dimension == name.text))
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

    /// Keep only `dimensions`, or `None` if one of them is unbound.
    ///
    /// Keep in step with `group_key`: two bindings must have equal keys for
    /// `dimensions` exactly when they project to equal bindings, as coverage
    /// and drop rules group by key and report the projection.
    pub fn project(&self, dimensions: &[String]) -> Option<Self> {
        // Keeping every dimension, as when grouping by all of them, is a copy.
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

    /// Its values for `dimensions`, in their order, or `None` if one of them
    /// is unbound: two bindings project onto `dimensions` alike exactly when
    /// their keys are equal, so bindings can be grouped without building a
    /// binding for each. Keep in step with `project`.
    pub(crate) fn group_key(&self, dimensions: &[String]) -> Option<GroupKey> {
        dimensions
            .iter()
            .map(|dimension| self.pair(dimension).map(|value| value.id))
            .collect::<Option<_>>()
            .map(GroupKey)
    }

    /// Compare values dimension by dimension in `dimensions` order, reading
    /// runs of digits as numbers, so `run=2` sorts before `run=10`.
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

/// A binding's values for some dimensions; see [`EntityBinding::group_key`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct GroupKey(Vec<u32>);

/// Order text as people read it: runs of digits compare by numeric value.
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

/// Why two unequal values are likely the same intended value.
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

/// An artifact as `product[dimension=value,...]`, its entities in the order
/// given.
pub(crate) fn identity<'a>(
    product: &str,
    entities: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> String {
    let mut text = String::with_capacity(product.len() + 32);
    push_identity(&mut text, product, entities);
    text
}

/// Add `product[dimension=value,...]` to `text`.
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductDef {
    pub name: String,
    pub artifact_type: ArtifactType,
    pub dimensions: Vec<String>,
}

impl ProductDef {
    pub fn new(
        name: impl Into<String>,
        artifact_type: ArtifactType,
        dimensions: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            dimensions: owned_strings(dimensions),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ArtifactInstance {
    pub product: String,
    pub artifact_type: ArtifactType,
    pub entities: EntityBinding,
}

/// An artifact's identity: its product and entity bindings, ignoring its type.
pub type ArtifactKey = (String, EntityBinding);

impl ArtifactInstance {
    pub fn key(&self) -> ArtifactKey {
        (self.product.clone(), self.entities.clone())
    }

    pub fn new(
        product: impl Into<String>,
        artifact_type: ArtifactType,
        entities: EntityBinding,
    ) -> Self {
        Self {
            product: product.into(),
            artifact_type,
            entities,
        }
    }
}

impl fmt::Display for ArtifactInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.entities.is_empty() {
            f.write_str(&self.product)
        } else {
            write!(f, "{}[{}]", self.product, self.entities)
        }
    }
}

impl ArtifactInstance {
    /// This artifact, borrowed.
    pub fn view(&self) -> Artifact<'_> {
        Artifact {
            product: &self.product,
            artifact_type: &self.artifact_type,
            entities: &self.entities,
        }
    }
}

/// An artifact borrowed from where it is kept, such as a DAG's
/// [`Artifacts`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Artifact<'a> {
    pub product: &'a str,
    pub artifact_type: &'a ArtifactType,
    pub entities: &'a EntityBinding,
}

impl Artifact<'_> {
    /// An owned copy.
    pub fn to_instance(self) -> ArtifactInstance {
        ArtifactInstance {
            product: self.product.to_owned(),
            artifact_type: self.artifact_type.clone(),
            entities: self.entities.clone(),
        }
    }
}

impl fmt::Display for Artifact<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.entities.is_empty() {
            f.write_str(self.product)
        } else {
            write!(f, "{}[{}]", self.product, self.entities)
        }
    }
}

/// An artifact's place in its DAG's [`Artifacts`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArtifactId(u32);

impl ArtifactId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Each artifact of a DAG, once, which its jobs refer to by [`ArtifactId`].
/// Every artifact of a product has the product's type, so the name and type
/// are kept once per product and each artifact holds its product's number
/// and its entities, a column each.
///
/// Keep in step with `index_producers` in `compile/definitions.rs`, which
/// rejects a product made by more than one step: that is why one type per
/// product holds. A product made by two steps could have two types.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Artifacts {
    products: Vec<(String, ArtifactType)>,
    product_numbers: FxHashMap<String, u32>,
    product: Vec<u32>,
    entities: Vec<EntityBinding>,
    ids: FxHashMap<(u32, EntityBinding), ArtifactId>,
}

impl Artifacts {
    /// How many artifacts there are.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn get(&self, id: ArtifactId) -> Artifact<'_> {
        let (product, artifact_type) = &self.products[self.product[id.index()] as usize];
        Artifact {
            product,
            artifact_type,
            entities: &self.entities[id.index()],
        }
    }

    /// The number of `id`'s product, which [`Artifacts::products`] lists.
    pub(crate) fn product_of(&self, id: ArtifactId) -> u32 {
        self.product[id.index()]
    }

    /// Each product's name and type, by number.
    pub(crate) fn products(&self) -> impl Iterator<Item = (&str, &ArtifactType)> {
        self.products
            .iter()
            .map(|(product, artifact_type)| (product.as_str(), artifact_type))
    }

    pub fn entities(&self, id: ArtifactId) -> &EntityBinding {
        &self.entities[id.index()]
    }

    /// The number of `product`, if it has any artifacts.
    pub(crate) fn product_number(&self, product: &str) -> Option<u32> {
        self.product_numbers.get(product).copied()
    }

    /// The artifact of `product` with `entities`, if there is one.
    pub fn find(&self, product: &str, entities: &EntityBinding) -> Option<ArtifactId> {
        let number = *self.product_numbers.get(product)?;
        self.ids.get(&(number, entities.clone())).copied()
    }

    /// Every artifact, in the order they were added.
    pub fn ids(&self) -> impl Iterator<Item = ArtifactId> {
        (0..self.len() as u32).map(ArtifactId)
    }

    /// The number of `product`, whose artifacts have `artifact_type`, adding
    /// it when it is new.
    pub(crate) fn product(&mut self, product: &str, artifact_type: &ArtifactType) -> u32 {
        if let Some(&number) = self.product_numbers.get(product) {
            return number;
        }
        let number = self.products.len() as u32;
        self.products
            .push((product.to_owned(), artifact_type.clone()));
        self.product_numbers.insert(product.to_owned(), number);
        number
    }

    /// Add the artifact of product number `product` with `entities`, or
    /// give the id of the one already there.
    pub(crate) fn add(
        &mut self,
        product: u32,
        entities: EntityBinding,
    ) -> Result<ArtifactId, ArtifactId> {
        let id = ArtifactId(
            u32::try_from(self.entities.len()).expect("fewer than 2^32 artifacts in a DAG"),
        );
        match self.ids.entry((product, entities)) {
            std::collections::hash_map::Entry::Occupied(entry) => Err(*entry.get()),
            std::collections::hash_map::Entry::Vacant(entry) => {
                self.product.push(product);
                self.entities.push(entry.key().1.clone());
                entry.insert(id);
                Ok(id)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cardinality {
    One,
    Many,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputPort {
    pub name: String,
    pub artifact_type: ArtifactType,
    pub cardinality: Cardinality,
}

impl InputPort {
    pub fn one(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            cardinality: Cardinality::One,
        }
    }

    pub fn many(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            cardinality: Cardinality::Many,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShapeRule {
    /// The input with every dimension the others join on drives one job per
    /// artifact, and the outputs keep its dimensions.
    Preserve,
    /// The one many-valued input groups by the dimension named in its `vary`
    /// binding; any single-artifact inputs are matched to each group.
    Aggregate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputPort {
    pub name: String,
    pub artifact_type: ArtifactType,
}

impl OutputPort {
    pub fn new(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
        }
    }
}

/// The port name of an operation's only, unnamed output.
pub(crate) const DEFAULT_OUTPUT: &str = "output";

/// A placeholder a command has without its operation naming the port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefaultPort {
    /// `{input}`: an operation's only input, when unnamed.
    Input,
    /// `{input1}`, `{input2}`, ...: the unnamed inputs of an operation with
    /// several, numbered from 1.
    InputAt(usize),
    /// `{inputs}`: an operation's only input when that is a many input,
    /// whatever its name.
    Inputs,
}

impl DefaultPort {
    /// The name SPIT gives the unnamed input at `index` of `count` inputs.
    pub fn for_input(index: usize, count: usize) -> Self {
        if count == 1 {
            Self::Input
        } else {
            Self::InputAt(index + 1)
        }
    }

    /// The name between the braces.
    pub fn name(self) -> String {
        match self {
            Self::Input => "input".to_owned(),
            Self::InputAt(number) => format!("input{number}"),
            Self::Inputs => "inputs".to_owned(),
        }
    }
}

/// Reads as the placeholder is written, such as `{input2}`.
impl fmt::Display for DefaultPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{}}}", self.name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationDef {
    pub name: String,
    pub inputs: Vec<InputPort>,
    /// Every artifact one job writes, in declaration order.
    pub outputs: Vec<OutputPort>,
    pub shape_rule: ShapeRule,
    /// Dimensions consumed by an aggregate operation.
    pub aggregated_dimensions: Vec<String>,
    /// The fewest artifacts the many input accepts in one job.
    pub minimum_collection: Option<usize>,
}

impl OperationDef {
    /// An operation with one output, named `output`.
    pub fn new(
        name: impl Into<String>,
        inputs: Vec<InputPort>,
        output_type: ArtifactType,
        shape_rule: ShapeRule,
    ) -> Self {
        Self::with_outputs(
            name,
            inputs,
            vec![OutputPort::new(DEFAULT_OUTPUT, output_type)],
            shape_rule,
        )
    }

    pub fn with_outputs(
        name: impl Into<String>,
        inputs: Vec<InputPort>,
        outputs: Vec<OutputPort>,
        shape_rule: ShapeRule,
    ) -> Self {
        Self {
            name: name.into(),
            inputs,
            outputs,
            shape_rule,
            aggregated_dimensions: Vec::new(),
            minimum_collection: None,
        }
    }

    #[must_use]
    pub fn aggregating(mut self, dimension: impl Into<String>) -> Self {
        self.aggregated_dimensions = vec![dimension.into()];
        self
    }

    #[must_use]
    pub fn aggregating_dimensions(
        mut self,
        dimensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.aggregated_dimensions = dimensions.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn at_least(mut self, minimum: usize) -> Self {
        self.minimum_collection = Some(minimum);
        self
    }
}

/// How a call binds one product to an operation input.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputBinding {
    pub product: String,
    /// `@ vary(dimensions)`: collect artifacts that differ in these dimensions.
    pub vary: Vec<String>,
    /// `@ where(dimension=value, ...)`: keep only artifacts with these values.
    /// A pinned dimension no longer takes part in matching.
    pub pinned: BTreeMap<String, String>,
    /// `@ same(dimension, ...)`: match the job on these dimensions only. Any
    /// other dimension must leave exactly one artifact for each job.
    pub same: Option<Vec<String>>,
    /// `@ each(dimension, ...)`: broadcast the input over these dimensions.
    /// The step runs once per value of them found in the product, and its
    /// outputs gain them.
    pub each: Vec<String>,
}

impl InputBinding {
    pub fn product(name: impl Into<String>) -> Self {
        Self {
            product: name.into(),
            ..Self::default()
        }
    }

    pub fn vary(product: impl Into<String>, dimension: impl Into<String>) -> Self {
        Self {
            vary: vec![dimension.into()],
            ..Self::product(product)
        }
    }

    pub fn varying(
        product: impl Into<String>,
        dimensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            vary: dimensions.into_iter().map(Into::into).collect(),
            ..Self::product(product)
        }
    }

    #[must_use]
    pub fn pin(mut self, dimension: impl Into<String>, value: impl Into<String>) -> Self {
        self.pinned.insert(dimension.into(), value.into());
        self
    }

    #[must_use]
    pub fn same_on(mut self, dimensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.same = Some(owned_strings(dimensions));
        self
    }

    #[must_use]
    pub fn each_of(mut self, dimensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.each = owned_strings(dimensions);
        self
    }

    pub fn product_name(&self) -> &str {
        &self.product
    }

    /// Whether any `@` selector is present.
    pub fn has_selectors(&self) -> bool {
        !self.vary.is_empty()
            || !self.pinned.is_empty()
            || self.same.is_some()
            || !self.each.is_empty()
    }

    /// The product's dimensions that remain after `where` pins some of them.
    pub fn free_dimensions(&self, dimensions: &[String]) -> Vec<String> {
        dimensions
            .iter()
            .filter(|dimension| !self.pinned.contains_key(*dimension))
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    pub operation: String,
    /// Bindings correspond to operation ports in declaration order.
    pub inputs: Vec<InputBinding>,
    /// Products, one per operation output port in declaration order.
    pub outputs: Vec<String>,
    /// The stage whose block holds this step, if any.
    pub stage: Option<String>,
}

impl Invocation {
    pub fn new(
        operation: impl Into<String>,
        inputs: Vec<InputBinding>,
        output_product: impl Into<String>,
    ) -> Self {
        Self::with_outputs(operation, inputs, [output_product])
    }

    pub fn with_outputs(
        operation: impl Into<String>,
        inputs: Vec<InputBinding>,
        outputs: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            operation: operation.into(),
            inputs,
            outputs: outputs.into_iter().map(Into::into).collect(),
            stage: None,
        }
    }

    #[must_use]
    pub fn in_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    /// The first output, which names the step in diagnostics.
    pub fn output_product(&self) -> &str {
        self.outputs.first().map_or("", String::as_str)
    }
}

/// What a command line does for its operation's jobs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandRole {
    /// Produce the job's outputs.
    Run,
    /// Check the job's inputs before it runs, such as their headers or grids;
    /// a nonzero exit stops the script.
    Verify,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandDef {
    pub operation: String,
    pub template: CommandTemplate,
    pub role: CommandRole,
}

impl CommandDef {
    pub fn new(operation: impl Into<String>, template: CommandTemplate) -> Self {
        Self {
            operation: operation.into(),
            template,
            role: CommandRole::Run,
        }
    }

    pub fn verify(operation: impl Into<String>, template: CommandTemplate) -> Self {
        Self {
            role: CommandRole::Verify,
            ..Self::new(operation, template)
        }
    }
}

/// A named group of steps, such as preprocessing or analysis. A stage owns
/// the products its steps assign; operations stay global. A nested stage's
/// name is its path, as in `preprocess/denoise`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageDef {
    pub name: String,
    /// The default path rule for the products of this stage and the stages
    /// nested in it that set none, in place of the pipeline's default.
    pub path_template: Option<PathTemplate>,
}

/// A directory pattern that discovers concrete entity bindings under a root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryDiscovery {
    pub name: String,
    pub dimensions: Vec<String>,
    pub template: PathTemplate,
}

impl StageDef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path_template: None,
        }
    }
}

/// The logical pipeline: what to make from which sources. It says nothing
/// about how a dataset's sources are found or filtered; see [`InputRules`].
#[derive(Clone, Debug, Default)]
pub struct Pipeline {
    pub products: Vec<ProductDef>,
    pub operations: Vec<OperationDef>,
    pub invocations: Vec<Invocation>,
    pub commands: Vec<CommandDef>,
    pub path_template: Option<PathTemplate>,
    pub product_paths: BTreeMap<String, PathTemplate>,
    /// Stages in declaration order.
    pub stages: Vec<StageDef>,
}

impl Pipeline {
    /// The stage of the step that produces `product`; `None` for a source or
    /// a step outside every stage.
    pub fn stage_of(&self, product: &str) -> Option<&str> {
        self.invocations
            .iter()
            .find(|invocation| invocation.outputs.iter().any(|output| output == product))
            .and_then(|invocation| invocation.stage.as_deref())
    }

    /// The path template `product` uses: its own rule, else its stage's
    /// default, else the pipeline's default.
    pub fn path_template_for(&self, product: &str) -> Option<&PathTemplate> {
        self.product_paths
            .get(product)
            .or_else(|| self.stage_path_template(product))
            .or(self.path_template.as_ref())
    }

    /// The default path rule of the stage that produces `product`, or of the
    /// nearest stage around it that sets one, with the stage that sets it.
    pub fn stage_path_rule(&self, product: &str) -> Option<(&str, &PathTemplate)> {
        let stage = self.stage_of(product)?;
        stage_and_parents(stage).find_map(|name| {
            self.stages
                .iter()
                .find(|candidate| candidate.name == name)
                .and_then(|stage| Some((stage.name.as_str(), stage.path_template.as_ref()?)))
        })
    }

    pub fn stage_path_template(&self, product: &str) -> Option<&PathTemplate> {
        self.stage_path_rule(product).map(|(_, template)| template)
    }

    /// Whether `product` is a source family, which no step produces.
    pub fn is_source(&self, product: &str) -> bool {
        self.products
            .iter()
            .any(|declared| declared.name == product)
            && !self
                .invocations
                .iter()
                .any(|invocation| invocation.outputs.iter().any(|output| output == product))
    }

    /// Each record of `inventory` as an artifact, by product and in order,
    /// after checking that it names a source, binds exactly its dimensions,
    /// and appears once.
    pub fn source_artifacts(
        &self,
        inventory: &SourceInventory,
    ) -> Result<BTreeMap<String, Vec<ArtifactInstance>>, ResolveError> {
        let mut artifacts: BTreeMap<String, Vec<ArtifactInstance>> = BTreeMap::new();
        let mut seen: FxHashSet<(&str, &EntityBinding)> = FxHashSet::default();
        self.check_sources(inventory, |product, record| {
            if !seen.insert((&record.product, &record.entities)) {
                return false;
            }
            artifacts
                .entry(record.product.clone())
                .or_default()
                .push(ArtifactInstance {
                    product: record.product.clone(),
                    artifact_type: product.artifact_type.clone(),
                    entities: record.entities.clone(),
                });
            true
        })?;
        for (name, family) in &mut artifacts {
            if let Some(product) = self.products.iter().find(|product| product.name == *name) {
                product.sort_family(family);
            }
        }
        Ok(artifacts)
    }

    /// Check each record of `inventory` in order: that it names a source
    /// and binds exactly its dimensions. `add` is given each record that
    /// passes, with its product, and says whether the record is new; one
    /// that is not is an error.
    pub(crate) fn check_sources<'a>(
        &'a self,
        inventory: &'a SourceInventory,
        mut add: impl FnMut(&'a ProductDef, &'a SourceRecord) -> bool,
    ) -> Result<(), ResolveError> {
        // Each product with the set of its dimensions, looked up once, not
        // once per record; the first declaration of a name wins, as when
        // searching.
        let mut products = FxHashMap::default();
        for product in &self.products {
            products.entry(product.name.as_str()).or_insert_with(|| {
                let dimensions: BTreeSet<_> =
                    product.dimensions.iter().map(String::as_str).collect();
                (product, dimensions)
            });
        }
        let produced: FxHashSet<_> = self
            .invocations
            .iter()
            .flat_map(|invocation| &invocation.outputs)
            .map(String::as_str)
            .collect();
        for record in &inventory.artifacts {
            let &(product, ref dimensions) =
                products.get(record.product.as_str()).ok_or_else(|| {
                    ResolveError::UnknownProduct {
                        name: record.product.clone(),
                    }
                })?;
            if produced.contains(record.product.as_str()) {
                return Err(ResolveError::InvalidDefinition {
                    subject: DefinitionSubject::Product(record.product.clone()),
                    detail: format!(
                        "product `{}` cannot be both a source family and an invocation output",
                        record.product
                    ),
                });
            }
            let source = || ArtifactInstance {
                product: record.product.clone(),
                artifact_type: product.artifact_type.clone(),
                entities: record.entities.clone(),
            };
            let binds_exactly = record.entities.len() == dimensions.len()
                && record
                    .entities
                    .dimensions()
                    .all(|name| dimensions.contains(name));
            if !binds_exactly {
                return Err(ResolveError::InvalidDefinition {
                    subject: DefinitionSubject::Source(record.clone()),
                    detail: format!(
                        "source `{}` must bind exactly the dimensions of product `{}`: [{}]",
                        source(),
                        product.name,
                        product.dimensions.join(", ")
                    ),
                });
            }
            if !add(product, record) {
                return Err(ResolveError::DuplicateSourceArtifact { artifact: source() });
            }
        }
        Ok(())
    }
}

impl ProductDef {
    /// Order this product's artifacts by its declared dimensions, reading
    /// numbers as numbers.
    pub fn sort_family(&self, family: &mut [ArtifactInstance]) {
        family.sort_by(|left, right| left.entities.cmp_in(&right.entities, &self.dimensions));
    }
}

/// How a dataset's sources are found and filtered: directory discovery,
/// `exclude`, `drop` and `require` rules, and where source files live. The input stage
/// reads these; job resolution never does.
#[derive(Clone, Debug, Default)]
pub struct InputRules {
    pub discoveries: Vec<DirectoryDiscovery>,
    /// `require` and `drop` rules, in declaration order.
    pub constraints: Vec<CoverageRule>,
    /// `exclude` rules, in declaration order, with each row of a file an
    /// `exclude from` line names in its place once the file is read.
    pub exclusions: Vec<Exclusion>,
    /// The files `exclude from` lines name, relative to the recipe's
    /// folder, with the line of each, until they are read.
    pub exclusion_files: Vec<(String, usize)>,
    /// Path rules for source products that the recipe, not the pipeline, sets.
    pub source_paths: BTreeMap<String, PathTemplate>,
}

impl InputRules {
    pub fn is_empty(&self) -> bool {
        self.discoveries.is_empty()
            && self.constraints.is_empty()
            && self.exclusions.is_empty()
            && self.exclusion_files.is_empty()
            && self.source_paths.is_empty()
    }

    /// The discovery rule named `name`, if any.
    pub fn discovery(&self, name: &str) -> Option<&DirectoryDiscovery> {
        self.discoveries.iter().find(|rule| rule.name == name)
    }
}

/// A stage's full name, then each stage around it: `a/b/c`, `a/b`, `a`.
pub(crate) fn stage_and_parents(stage: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(stage), |name| {
        name.rsplit_once('/').map(|(parent, _)| parent)
    })
}

/// Whether `stage` is `outer` or a stage nested inside it.
pub fn stage_within(stage: &str, outer: &str) -> bool {
    stage
        .strip_prefix(outer)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// An `exclude` rule: it removes every source artifact whose identity
/// includes each value it names, of its product or, when it names none, of
/// every product, with every discovered context that does too.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Exclusion {
    pub product: Option<String>,
    /// Each dimension and value it names, in the order written.
    pub values: Vec<(String, String)>,
    /// Why, from the comment on its line or a file's `reason` column.
    pub reason: Option<String>,
    /// Where it is written: `line 4` of the recipe, or `qc/excluded.csv row
    /// 3` for a row of a file an `exclude from` line names.
    pub origin: String,
}

impl Exclusion {
    /// Whether it removes the artifact of `product` with `entities`.
    pub fn matches(&self, product: &str, entities: &EntityBinding) -> bool {
        self.product.as_deref().is_none_or(|name| name == product) && self.within(entities)
    }

    /// Whether it removes the discovered context `binding`: only a rule that
    /// names no product removes contexts.
    pub fn matches_context(&self, binding: &EntityBinding) -> bool {
        self.product.is_none() && self.within(binding)
    }

    /// Whether `entities` has every value this rule names.
    fn within(&self, entities: &EntityBinding) -> bool {
        self.values
            .iter()
            .all(|(dimension, value)| entities.get(dimension) == Some(value.as_str()))
    }

    /// The values it names, as a binding.
    pub fn binding(&self) -> EntityBinding {
        self.values.iter().cloned().collect()
    }

    /// What it names, as written: `bold[sub=02,run=3]`, `[store=s07]`, or
    /// `testset`.
    pub fn pattern(&self) -> String {
        let mut text = self.product.clone().unwrap_or_default();
        if !self.values.is_empty() || self.product.is_none() {
            let pairs = self
                .values
                .iter()
                .map(|(dimension, value)| (dimension.as_str(), value.as_str()));
            push_bindings(&mut text, pairs);
        }
        text
    }
}

impl fmt::Display for Exclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exclude {}", self.pattern())
    }
}

/// What the input stage left out of a dataset, and the rule that did: an
/// artifact of `product`, or, with no product, a group, every artifact
/// whose identity includes `entities`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Removal {
    pub product: Option<String>,
    pub entities: EntityBinding,
    /// The rule, as `exclude bold[run=3]` or `drop [sub] where sessions
    /// count<2`.
    pub rule: String,
    /// Where the rule is written, when known: `line 4` of the recipe, or a
    /// line of a file.
    pub origin: Option<String>,
    pub reason: Option<String>,
    /// For a group a `drop` rule counted: how many it found.
    pub found: Option<usize>,
}

impl Removal {
    /// What was removed, as `bold[run=3,sub=02]` or `[sub=03]`, its
    /// dimensions in name order.
    pub fn identity(&self) -> String {
        self.identity_in(&[])
    }

    /// As [`Removal::identity`], with the dimensions `declared` names first,
    /// in that order.
    pub fn identity_in(&self, declared: &[String]) -> String {
        let mut pairs: Vec<_> = self.entities.iter().collect();
        pairs.sort_by_key(|(dimension, _)| {
            declared
                .iter()
                .position(|name| name == dimension)
                .unwrap_or(usize::MAX)
        });
        let mut text = self.product.clone().unwrap_or_default();
        if !pairs.is_empty() || self.product.is_none() {
            push_bindings(&mut text, pairs.into_iter());
        }
        text
    }

    /// Whether an `exclude` rule made it, rather than a `drop` rule.
    pub fn is_exclusion(&self) -> bool {
        self.rule.starts_with("exclude")
    }
}

/// Add `[dimension=value,...]` to `text`.
fn push_bindings<'a>(text: &mut String, pairs: impl Iterator<Item = (&'a str, &'a str)>) {
    text.push('[');
    for (index, (dimension, value)) in pairs.enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(dimension);
        text.push('=');
        text.push_str(value);
    }
    text.push(']');
}

/// A source record identifies a logical artifact, and may say where its file
/// is, relative to the dataset root, as the input stage found it.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SourceRecord {
    pub product: String,
    pub entities: EntityBinding,
    pub path: Option<String>,
}

impl SourceRecord {
    pub fn new(product: impl Into<String>, entities: EntityBinding) -> Self {
        Self {
            product: product.into(),
            entities,
            path: None,
        }
    }

    /// This record with its file at `path`, relative to the dataset root.
    #[must_use]
    pub fn at(self, path: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            ..self
        }
    }
}

/// Supplied by a dataset indexer, a manifest, or the text fixture parser.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceInventory {
    pub artifacts: Vec<SourceRecord>,
    /// Observed contexts can expose missing artifacts even when no other source
    /// family has an artifact for that context.
    pub contexts: Vec<EntityBinding>,
    /// Bindings from each named directory discovery rule. These are also
    /// present in `contexts`, but retain their origin for coverage rules.
    pub discovered: BTreeMap<String, Vec<EntityBinding>>,
    /// Source path rules settled from a recipe, when the pipeline does not
    /// declare them. A .spitout carries each rule once for standalone DAGs.
    pub source_paths: BTreeMap<String, PathTemplate>,
    /// What the input stage left out, and why: a record, not a rule, so
    /// resolving jobs removes nothing more for it.
    pub removed: Vec<Removal>,
}

/// A comparison of how many artifacts or contexts a group holds, as a
/// rule writes it after `count`: `=2`, `!=2`, `>=2`, `<=2`, `>2` or `<2`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountRequirement {
    Exactly(usize),
    NotExactly(usize),
    AtLeast(usize),
    AtMost(usize),
    MoreThan(usize),
    FewerThan(usize),
}

impl CountRequirement {
    /// Whether `found` artifacts or bindings meet the requirement.
    pub fn allows(&self, found: usize) -> bool {
        match *self {
            Self::Exactly(count) => found == count,
            Self::NotExactly(count) => found != count,
            Self::AtLeast(count) => found >= count,
            Self::AtMost(count) => found <= count,
            Self::MoreThan(count) => found > count,
            Self::FewerThan(count) => found < count,
        }
    }

    /// The comparison as a rule writes it: `count>=2`.
    pub fn as_written(&self) -> String {
        let (op, count) = match *self {
            Self::Exactly(count) => ("=", count),
            Self::NotExactly(count) => ("!=", count),
            Self::AtLeast(count) => (">=", count),
            Self::AtMost(count) => ("<=", count),
            Self::MoreThan(count) => (">", count),
            Self::FewerThan(count) => ("<", count),
        };
        format!("count{op}{count}")
    }

    /// The comparison that holds exactly when this one does not.
    pub fn negated(&self) -> Self {
        match *self {
            Self::Exactly(count) => Self::NotExactly(count),
            Self::NotExactly(count) => Self::Exactly(count),
            Self::AtLeast(count) => Self::FewerThan(count),
            Self::AtMost(count) => Self::MoreThan(count),
            Self::MoreThan(count) => Self::AtMost(count),
            Self::FewerThan(count) => Self::AtLeast(count),
        }
    }
}

impl fmt::Display for CountRequirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exactly(count) => write!(f, "exactly {count}"),
            Self::NotExactly(count) => write!(f, "other than {count}"),
            Self::AtLeast(count) => write!(f, "at least {count}"),
            Self::AtMost(count) => write!(f, "at most {count}"),
            Self::MoreThan(count) => write!(f, "more than {count}"),
            Self::FewerThan(count) => write!(f, "fewer than {count}"),
        }
    }
}

/// A `require` or `drop` rule over the groups of a dataset that `group_by`
/// forms, counting the artifacts of a source, or the contexts of a
/// discovery rule, in each: `product`.
///
/// A `require` rule fails a group unless its count holds and it has every
/// value in `values`. A `drop` rule removes a group when its count holds,
/// when it lacks a value in `values`, or when it has a value in `has`; it
/// names one of the three.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageRule {
    pub action: CoverageAction,
    pub product: String,
    pub group_by: Vec<String>,
    /// The count; a `require` rule without one needs at least one.
    pub count: Option<CountRequirement>,
    /// Entity values that must each be present in every group, such as
    /// `run=1,2`. Each listed dimension is checked on its own.
    pub values: BTreeMap<String, Vec<String>>,
    /// For `drop … has`: values any one of which removes a group.
    pub has: BTreeMap<String, Vec<String>>,
    /// The recipe line the rule is written on, when known.
    pub line: Option<usize>,
}

/// Reads as written: `drop [sub] where sessions count<2`, or `require
/// image run=1,2 per [sub]`.
impl fmt::Display for CoverageRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let clause = |values: &BTreeMap<String, Vec<String>>| {
            values
                .iter()
                .map(|(dimension, values)| format!("{dimension}={}", values.join(",")))
                .collect::<Vec<_>>()
                .join(" ")
        };
        match self.action {
            CoverageAction::Require => {
                write!(f, "require {}", self.product)?;
                if let Some(count) = self.count {
                    write!(f, " {}", count.as_written())?;
                }
                if !self.values.is_empty() {
                    write!(f, " {}", clause(&self.values))?;
                }
                write!(f, " per [{}]", self.group_by.join(", "))
            }
            CoverageAction::Drop => {
                write!(
                    f,
                    "drop [{}] where {}",
                    self.group_by.join(", "),
                    self.product
                )?;
                if let Some(count) = self.count {
                    write!(f, " {}", count.as_written())?;
                }
                if !self.values.is_empty() {
                    write!(f, " missing {}", clause(&self.values))?;
                }
                if !self.has.is_empty() {
                    write!(f, " has {}", clause(&self.has))?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoverageAction {
    #[default]
    Require,
    Drop,
}

impl CoverageRule {
    pub fn new(
        product: impl Into<String>,
        group_by: impl IntoIterator<Item = impl AsRef<str>>,
        count: CountRequirement,
    ) -> Self {
        Self {
            action: CoverageAction::Require,
            product: product.into(),
            group_by: owned_strings(group_by),
            count: Some(count),
            values: BTreeMap::new(),
            has: BTreeMap::new(),
            line: None,
        }
    }

    /// Whether a group whose members have `bindings` passes a `require`
    /// rule, or is removed by a `drop` rule.
    pub fn holds_for(&self, bindings: &[&EntityBinding]) -> bool {
        let lacks = |values: &BTreeMap<String, Vec<String>>| {
            values.iter().any(|(dimension, listed)| {
                listed.iter().any(|value| {
                    !bindings
                        .iter()
                        .any(|binding| binding.get(dimension) == Some(value.as_str()))
                })
            })
        };
        match self.action {
            CoverageAction::Require => {
                self.count
                    .unwrap_or(CountRequirement::AtLeast(1))
                    .allows(bindings.len())
                    && !lacks(&self.values)
            }
            CoverageAction::Drop => {
                let has = self.has.iter().any(|(dimension, listed)| {
                    bindings.iter().any(|binding| {
                        binding
                            .get(dimension)
                            .is_some_and(|found| listed.iter().any(|value| value == found))
                    })
                });
                self.count.is_some_and(|count| count.allows(bindings.len()))
                    || lacks(&self.values)
                    || has
            }
        }
    }

    #[must_use]
    pub fn requiring(
        mut self,
        dimension: impl Into<String>,
        values: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.values.insert(dimension.into(), owned_strings(values));
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Job {
    pub id: usize,
    pub operation: String,
    /// The artifacts bound to each input port, in port order. A many port
    /// holds its collection in order; every other port holds one artifact.
    pub inputs: Vec<Vec<ArtifactId>>,
    /// One artifact per output port, in port order.
    pub outputs: Vec<ArtifactId>,
    pub dependencies: Vec<usize>,
    /// The stage of the step that made this job, if any.
    pub stage: Option<String>,
}

impl Job {
    /// The first output artifact.
    ///
    /// # Panics
    ///
    /// If the job has no outputs. A resolved job always has one, since
    /// every operation declares at least one output.
    pub fn output(&self) -> ArtifactId {
        self.outputs[0]
    }

    /// Every input artifact, in port order.
    pub fn input_artifacts(&self) -> impl Iterator<Item = ArtifactId> + '_ {
        self.inputs.iter().flatten().copied()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedDag {
    pub jobs: Vec<Job>,
    /// Every artifact the jobs refer to.
    pub artifacts: Artifacts,
    /// Declaration order is retained for readable dry-run output.
    pub product_dimensions: BTreeMap<String, Vec<String>>,
    /// The file of each source whose inventory record gave one, by artifact
    /// id. Other artifacts take the path their product's rule gives them.
    pub(crate) source_paths: Vec<Option<String>>,
    /// The products of the inventory records that gave a file.
    pub(crate) located: BTreeSet<String>,
}

impl ResolvedDag {
    pub fn artifact(&self, id: ArtifactId) -> Artifact<'_> {
        self.artifacts.get(id)
    }

    /// Take each source's file from its record in `inventory`, replacing the
    /// files known before: a DAG resolved before its inventory's sources
    /// were located gets their files this way, without resolving it again.
    pub fn locate_sources(&mut self, inventory: &SourceInventory) {
        self.source_paths = vec![None; self.artifacts.len()];
        self.located.clear();
        for record in &inventory.artifacts {
            let Some(path) = &record.path else {
                continue;
            };
            if !self.located.contains(&record.product) {
                self.located.insert(record.product.clone());
            }
            if let Some(id) = self.artifacts.find(&record.product, &record.entities) {
                self.source_paths[id.index()] = Some(path.clone());
            }
        }
    }

    /// The file `id`'s inventory record gave it, if it is a source with one.
    pub fn source_path(&self, id: ArtifactId) -> Option<&str> {
        self.source_paths.get(id.index())?.as_deref()
    }

    /// The products with a source whose inventory record gave its file.
    pub fn located_products(&self) -> impl Iterator<Item = &str> {
        self.located.iter().map(String::as_str)
    }

    /// Only the jobs of `stage` and the stages nested in it. Their inputs from
    /// other stages are taken as files that already exist, so dependencies on those jobs are dropped;
    /// every job keeps its number.
    #[must_use]
    pub fn only_stage(&self, stage: &str) -> Self {
        let jobs: Vec<_> = self
            .jobs
            .iter()
            .filter(|job| {
                job.stage
                    .as_deref()
                    .is_some_and(|name| stage_within(name, stage))
            })
            .cloned()
            .collect();
        let kept: BTreeSet<_> = jobs.iter().map(|job| job.id).collect();
        Self {
            jobs: jobs
                .into_iter()
                .map(|mut job| {
                    job.dependencies.retain(|id| kept.contains(id));
                    job
                })
                .collect(),
            artifacts: self.artifacts.clone(),
            product_dimensions: self.product_dimensions.clone(),
            source_paths: self.source_paths.clone(),
            located: self.located.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Gap {
    Unmatched(ResolveError),
    /// An input is the output of an incomplete job, or a source held back by
    /// a coverage gap.
    Blocked {
        port: String,
        artifact: ArtifactInstance,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncompleteJob {
    pub operation: String,
    pub stage: Option<String>,
    /// The inputs it did match, by id in the DAG's table, in port order.
    pub inputs: Vec<ArtifactId>,
    pub outputs: Vec<ArtifactInstance>,
    pub gaps: Vec<Gap>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageGap {
    pub error: ResolveError,
    pub sources: Vec<ArtifactInstance>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArtifactReport {
    /// Every source, by product in declaration order.
    pub sources: Vec<ArtifactId>,
    pub dag: ResolvedDag,
    pub incomplete: Vec<IncompleteJob>,
    pub coverage: Vec<CoverageGap>,
}

impl ArtifactReport {
    /// The sources no job reads, complete or not, in source order: files
    /// the inventory holds that the plan has no use for, such as those a
    /// `where` selector leaves out, or a file whose name matches no value a
    /// job needs. Sources a coverage gap holds back are reported with the
    /// gap, not here.
    pub fn unused_sources(&self) -> Vec<ArtifactId> {
        let mut read = vec![false; self.dag.artifacts.len()];
        let complete = self.dag.jobs.iter().flat_map(Job::input_artifacts);
        let incomplete = self
            .incomplete
            .iter()
            .flat_map(|job| job.inputs.iter().copied());
        for id in complete.chain(incomplete) {
            read[id.index()] = true;
        }
        for source in self.coverage.iter().flat_map(|gap| &gap.sources) {
            if let Some(id) = self.dag.artifacts.find(&source.product, &source.entities) {
                read[id.index()] = true;
            }
        }
        self.sources
            .iter()
            .copied()
            .filter(|id| !read[id.index()])
            .collect()
    }
}

fn owned_strings(values: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect()
}
