//! A compiled pipeline: what to make from which sources.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::paths::{EntitiesFormat, PathTemplate};

use super::{
    ArtifactInstance, Call, CallId, CheckDef, CommandDef, DefaultChecks, EntityBinding,
    ExtensionSource, Invocation, OperationDef, PipelineIndex, ProductDef, Removal, SidecarGroup,
    SourceInventory, SourceRecord, StageDef,
};

/// The logical pipeline: what to make from which sources. It says nothing
/// about how a dataset's sources are filtered; see [`InputRules`].
#[derive(Clone, Debug, Default)]
pub struct Pipeline {
    /// An optional dataset root and its line, relative to the pipeline's
    /// folder when parsed at a path. Imports never bring in a root.
    pub root: Option<(PathBuf, usize)>,
    pub products: Vec<ProductDef>,
    pub operations: Vec<OperationDef>,
    pub invocations: Vec<Invocation>,
    pub commands: Vec<CommandDef>,
    /// `check` declarations, in declaration order.
    pub checks: Vec<CheckDef>,
    /// The `check:` default of every output in the file, outside any stage.
    pub default_checks: DefaultChecks,
    pub path_template: Option<PathTemplate>,
    /// How `{@entities}` is written, from the `default entities:` line
    /// outside every stage. A product's spelling is its stage's, else this
    /// one; a rule that uses `{@entities}` for a product with neither is an
    /// error.
    pub entities_format: Option<EntitiesFormat>,
    /// The `ext:` default: the extension a default path rule is completed
    /// with when the operation declares none.
    pub extension: Option<String>,
    pub product_paths: BTreeMap<String, PathTemplate>,
    /// Stages in declaration order.
    pub stages: Vec<StageDef>,
    /// Source families joined by `beside`, for reporting missing companions.
    pub sidecar_groups: Vec<SidecarGroup>,
    /// Each call to an operation with a body, which `invocations` holds as
    /// the body's steps; a step's [`Invocation::origin`] names its call.
    pub calls: Vec<Call>,
    /// The files the pipeline was read from: its own first, then each file
    /// an import read, once. Empty for a pipeline parsed without a path.
    pub files: Vec<SourceFile>,
    /// How much of the pipeline must be typed, from its `types` line.
    /// Imports never bring one: the file given to SPIT decides.
    pub types: TypeChecking,
    /// Each type declared with `type Name<Param, ...>`, and its parameter
    /// names in order. A declared type is written with that many arguments.
    pub type_decls: BTreeMap<String, Vec<String>>,
}

/// How much of a pipeline must be typed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TypeChecking {
    /// Types are optional: a type that is not known is not checked.
    #[default]
    Standard,
    /// Every source, every port of an operation a step or call uses, and
    /// every product a step makes has a type with no unknown part, once
    /// inference is done.
    Strict,
}

/// A file a pipeline was read from.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceFile {
    /// Relative to the pipeline's folder, with `/` between folders; the
    /// pipeline's own file is its name.
    pub path: String,
    /// Its git blob id, as `git hash-object` prints it.
    pub blob: String,
}

impl Pipeline {
    /// The call written in the pipeline that `call` was made by: `call`
    /// itself, or the outermost of the calls it is nested in.
    pub fn written_call(&self, call: CallId) -> &Call {
        &self.calls[self.written_call_id(call).index()]
    }

    /// The id of [`Pipeline::written_call`].
    pub fn written_call_id(&self, mut call: CallId) -> CallId {
        while let Some(parent) = self.calls[call.index()].parent {
            call = parent;
        }
        call
    }

    /// The position in [`Pipeline::files`] of the file declaring each
    /// operation, by name, when the pipeline was read from files: the
    /// operation's `file`, or else the pipeline's own, the first.
    pub fn operation_files(&self) -> FxHashMap<&str, Option<usize>> {
        let at: FxHashMap<&str, usize> = self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| (file.path.as_str(), index))
            .collect();
        self.operations
            .iter()
            .map(|operation| {
                let file = match &operation.file {
                    Some(file) => at.get(file.as_str()).copied(),
                    None => (!self.files.is_empty()).then_some(0),
                };
                (operation.name.as_str(), file)
            })
            .collect()
    }
}

impl Pipeline {
    /// The pipeline's dimensions, each once, in the order its products
    /// declare them: how the `.spitout` and a note about what was removed
    /// write a group.
    pub fn dimension_order(&self) -> Vec<String> {
        let mut seen = FxHashSet::default();
        self.products
            .iter()
            .flat_map(|product| &product.dimensions)
            .filter(|dimension| seen.insert(dimension.as_str()))
            .cloned()
            .collect()
    }

    /// The stage of the step that produces `product`; `None` for a source or
    /// a step outside every stage.
    pub fn stage_of(&self, product: &str) -> Option<&str> {
        PipelineIndex::scan(self).stage_of(product)
    }

    /// The path template `product` uses: its rule, with the extension
    /// [`Pipeline::added_extension`] gives it, or, for an output written
    /// beside another, that output's path with its extension replaced.
    pub fn path_template_for(&self, product: &str) -> Option<Cow<'_, PathTemplate>> {
        PipelineIndex::scan(self).path_template_for(product)
    }

    /// The path rule `product` uses, as written: its own rule, else its
    /// stage's default, else the pipeline's default, unless that needs
    /// `{@stage}` and `product` is a source. An output in a pipeline with
    /// no default takes the built-in one.
    pub fn path_rule_for(&self, product: &str) -> Option<&PathTemplate> {
        PipelineIndex::scan(self).path_rule_for(product)
    }

    /// The operation that makes `product`, with the extension it declares
    /// for that output. An output written beside another has none to
    /// complete a rule with: its path follows the other's.
    pub fn output_extension(&self, product: &str) -> Option<(&str, &str)> {
        PipelineIndex::scan(self).output_extension(product)
    }

    /// For a source or output written beside another, its sibling, the
    /// sibling's declared extension, and the suffix that replaces it.
    pub fn beside(&self, product: &str) -> Option<(&str, &str, &str)> {
        PipelineIndex::scan(self).beside(product)
    }

    /// The `ext:` default for `product`: its stage's, or the nearest
    /// enclosing stage's, else the pipeline's.
    pub fn default_extension(&self, product: &str) -> Option<(&str, ExtensionSource)> {
        PipelineIndex::scan(self).default_extension(product)
    }

    /// The extension `product`'s file must have, and where it is declared:
    /// its operation's, or the one a source declares, else, when its path
    /// is a default rule, the `ext:` default. A product with its own rule
    /// takes only its operation's or its declared one.
    pub fn expected_extension(&self, product: &str) -> Option<(&str, ExtensionSource)> {
        PipelineIndex::scan(self).expected_extension(product)
    }

    /// The extension added to `product`'s path rule: the one it must have,
    /// when the rule ends without an extension. A rule that ends with
    /// another is left as written, and reported by the path checks.
    pub fn added_extension(&self, product: &str) -> Option<(&str, ExtensionSource)> {
        PipelineIndex::scan(self).added_extension(product)
    }

    /// The default path rule of the stage that produces `product`, or of the
    /// nearest stage around it that sets one, with the stage that sets it.
    pub fn stage_path_rule(&self, product: &str) -> Option<(&str, &PathTemplate)> {
        PipelineIndex::scan(self).stage_path_rule(product)
    }

    pub fn stage_path_template(&self, product: &str) -> Option<&PathTemplate> {
        self.stage_path_rule(product).map(|(_, template)| template)
    }

    /// The source and companions anchored by `name`.
    pub fn sidecar_group(&self, name: &str) -> Option<&SidecarGroup> {
        self.sidecar_groups.iter().find(|group| group.name == name)
    }

    /// Each member of a companion group, with its group, to find once and
    /// then look up.
    pub fn sidecar_members(&self) -> FxHashMap<&str, &SidecarGroup> {
        self.sidecar_groups
            .iter()
            .flat_map(|group| {
                group
                    .members
                    .iter()
                    .map(move |(member, _)| (member.as_str(), group))
            })
            .collect()
    }

    /// Whether `product`'s artifacts are folders: its operation's output
    /// says so, or, for a source, its declaration.
    pub fn is_folder(&self, product: &str) -> bool {
        PipelineIndex::scan(self).is_folder(product)
    }

    /// Whether `product` is a source family, which no step produces.
    pub fn is_source(&self, product: &str) -> bool {
        PipelineIndex::scan(self).is_source(product)
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

/// The order a pipeline's dimensions are written in, to write what an input
/// rule removed: an artifact's in its product's order, and a group's in the
/// order the pipeline first declares them, as the `.spitout` writes it. Found
/// once, so that writing every removal takes time in step with their number.
pub struct DimensionOrders<'p> {
    products: FxHashMap<&'p str, &'p [String]>,
    pipeline: Vec<String>,
}

impl<'p> DimensionOrders<'p> {
    pub fn new(pipeline: &'p Pipeline) -> Self {
        let mut products = FxHashMap::default();
        for product in &pipeline.products {
            // The first of a repeated name wins, as a search finds it.
            products
                .entry(product.name.as_str())
                .or_insert(product.dimensions.as_slice());
        }
        Self {
            products,
            pipeline: pipeline.dimension_order(),
        }
    }

    /// The dimensions `removal` is written in.
    pub fn of(&self, removal: &Removal) -> &[String] {
        removal
            .product
            .as_deref()
            .and_then(|name| self.products.get(name).copied())
            .unwrap_or(&self.pipeline)
    }
}
