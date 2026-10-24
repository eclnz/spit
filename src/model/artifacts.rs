//! Artifacts: one product's file for one binding, and the table a DAG
//! keeps each of them in once.

use std::fmt;

use rustc_hash::FxHashMap;

use crate::types::TypeExpr;

use super::EntityBinding;

pub type ArtifactType = TypeExpr;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ArtifactInstance {
    pub product: String,
    pub artifact_type: ArtifactType,
    pub entities: EntityBinding,
}

/// An artifact's identity: its product and entity bindings, ignoring its type.
pub type ArtifactKey = (String, EntityBinding);

impl ArtifactInstance {
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

    /// This artifact, borrowed.
    pub fn view(&self) -> Artifact<'_> {
        Artifact {
            product: &self.product,
            artifact_type: &self.artifact_type,
            entities: &self.entities,
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

/// An artifact borrowed from where it is kept, such as a DAG's
/// [`Artifacts`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Artifact<'a> {
    pub product: &'a str,
    pub artifact_type: &'a ArtifactType,
    pub entities: &'a EntityBinding,
}

impl<'a> Artifact<'a> {
    /// Its product and entities, which identify it, borrowed.
    pub fn key(self) -> (&'a str, &'a EntityBinding) {
        (self.product, self.entities)
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(sub: &str) -> EntityBinding {
        EntityBinding::from_pairs([("sub", sub)])
    }

    #[test]
    fn adding_an_artifact_twice_gives_its_id_and_keeps_one_row() {
        let mut artifacts = Artifacts::default();
        let t = artifacts.product("raw", &TypeExpr::named("T"));
        let first = artifacts.add(t, binding("1")).unwrap();
        assert_eq!(artifacts.add(t, binding("1")), Err(first));
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts.ids().count(), 1);
    }

    #[test]
    fn a_product_is_numbered_once_and_keeps_the_first_type_it_is_given() {
        let mut artifacts = Artifacts::default();
        let raw = artifacts.product("raw", &TypeExpr::named("T"));
        let other = artifacts.product("other", &TypeExpr::named("U"));
        assert_eq!((raw, other), (0, 1));
        assert_eq!(artifacts.product("raw", &TypeExpr::named("U")), raw);
        let listed: Vec<_> = artifacts.products().collect();
        assert_eq!(
            listed,
            [
                ("raw", &TypeExpr::named("T")),
                ("other", &TypeExpr::named("U"))
            ]
        );
        assert_eq!(artifacts.product_number("raw"), Some(0));
        assert_eq!(artifacts.product_number("missing"), None);
    }

    #[test]
    fn find_gives_what_add_added_and_nothing_else() {
        let mut artifacts = Artifacts::default();
        let raw = artifacts.product("raw", &TypeExpr::named("T"));
        let other = artifacts.product("other", &TypeExpr::named("T"));
        let id = artifacts.add(raw, binding("1")).unwrap();
        assert_eq!(artifacts.find("raw", &binding("1")), Some(id));
        assert_eq!(artifacts.find("raw", &binding("2")), None);
        assert_eq!(artifacts.find("other", &binding("1")), None);
        assert_eq!(artifacts.find("missing", &binding("1")), None);
        // A product with the same entities is another artifact.
        let same_entities = artifacts.add(other, binding("1")).unwrap();
        assert_ne!(same_entities, id);
        assert_eq!(artifacts.find("other", &binding("1")), Some(same_entities));
    }

    #[test]
    fn ids_count_up_from_zero_in_the_order_artifacts_are_added() {
        let mut artifacts = Artifacts::default();
        let raw = artifacts.product("raw", &TypeExpr::named("T"));
        let added = [binding("3"), binding("1"), binding("2")]
            .map(|entities| artifacts.add(raw, entities).unwrap());
        // A repeat takes no id.
        artifacts.add(raw, binding("1")).unwrap_err();
        let ids: Vec<_> = artifacts.ids().collect();
        assert_eq!(ids, added);
        let indexes: Vec<_> = ids.iter().map(|id| id.index()).collect();
        assert_eq!(indexes, [0, 1, 2]);
        assert_eq!(artifacts.get(ids[0]).entities, &binding("3"));
        assert_eq!(
            artifacts.get(ids[2]).to_instance().to_string(),
            "raw[sub=2]"
        );
    }
}
