//! A compiled pipeline, and an index that finds each product's step once.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::paths::{EntitiesFormat, Holder, PathTemplate};

use super::{
    stage_and_parents, ArtifactInstance, Call, CallId, CheckDef, CommandDef, DefaultChecks,
    EntityBinding, ExtensionSource, Invocation, OperationDef, OutputPort, ProductDef, Removal,
    SidecarGroup, SourceInventory, SourceRecord, StageDef,
};

/// The logical pipeline: what to make from which sources. It says nothing
/// about how a dataset's sources are found or filtered; see [`InputRules`].
#[derive(Clone, Debug, Default)]
pub struct Pipeline {
    pub products: Vec<ProductDef>,
    pub operations: Vec<OperationDef>,
    pub invocations: Vec<Invocation>,
    pub commands: Vec<CommandDef>,
    /// `check` declarations, in declaration order.
    pub checks: Vec<CheckDef>,
    /// The `check:` default of every output in the file, outside any stage.
    pub default_checks: DefaultChecks,
    pub path_template: Option<PathTemplate>,
    /// The file's custom rendering of `{@entities}`; None keeps the built-in format.
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

/// A product's place in [`Pipeline::products`], by which the columns of a
/// [`PipelineIndex`] and of lowering hold what belongs to each product.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProductId(u32);

impl ProductId {
    /// The product at `index` in [`Pipeline::products`].
    pub(crate) fn at(index: usize) -> Self {
        Self(u32::try_from(index).expect("fewer than 2^32 products in a pipeline"))
    }

    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// A pipeline, for asking where many products are made and what paths they
/// take. Each answer needs the step that makes a product; one built with
/// [`PipelineIndex::new`] numbers each product once and keeps its producer
/// in a column by [`ProductId`], so asking about every product takes time in
/// step with the pipeline, not its square. One
/// from [`PipelineIndex::scan`] searches each time, which costs no more for
/// a single question.
pub(crate) struct PipelineIndex<'p> {
    pub(crate) pipeline: &'p Pipeline,
    found: Option<Found<'p>>,
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

/// Where the path rule of a product comes from, in the order a product looks
/// for one. Keep in step with [`PipelineIndex::path_origin`], the only place
/// that chooses, and with the places that say so: the editor's hover, the
/// `--path-rules` listing and the source map's line for the rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PathOrigin<'p> {
    /// The product's own `path product:` rule.
    Explicit,
    /// The default of the named stage, or of the nearest stage around it
    /// that sets one.
    Stage(&'p str),
    /// The pipeline's `path:` default.
    Default,
    /// The built-in default, `out/{@product}/{@entities}`, for an output in
    /// a pipeline with no `path:` default.
    BuiltIn,
}

/// What a [`PipelineIndex::new`] finds once.
struct Found<'p> {
    /// Each product's id, by name.
    ids: FxHashMap<&'p str, ProductId>,
    /// Each operation by name.
    operations: FxHashMap<&'p str, &'p OperationDef>,
    /// The step that makes each product, and the output's position, by id.
    producers: Vec<Option<(&'p Invocation, usize)>>,
}

impl<'p> PipelineIndex<'p> {
    /// `pipeline` with its products, operations and producers found once.
    pub(crate) fn new(pipeline: &'p Pipeline) -> Self {
        // The first of any repeat wins, as a search finds it; repeats are
        // reported elsewhere.
        let mut ids = FxHashMap::default();
        for (index, product) in pipeline.products.iter().enumerate() {
            ids.entry(product.name.as_str())
                .or_insert(ProductId::at(index));
        }
        let mut operations = FxHashMap::default();
        for operation in &pipeline.operations {
            operations
                .entry(operation.name.as_str())
                .or_insert(operation);
        }
        let mut producers = vec![None; pipeline.products.len()];
        for invocation in &pipeline.invocations {
            for (index, output) in invocation.outputs.iter().enumerate() {
                if let Some(id) = ids.get(output.as_str()) {
                    producers[id.index()].get_or_insert((invocation, index));
                }
            }
        }
        Self {
            pipeline,
            found: Some(Found {
                ids,
                operations,
                producers,
            }),
        }
    }

    /// `pipeline`, searched for each question.
    pub(crate) fn scan(pipeline: &'p Pipeline) -> Self {
        Self {
            pipeline,
            found: None,
        }
    }

    /// `product`'s id, the first of its name.
    pub(crate) fn id(&self, product: &str) -> Option<ProductId> {
        match &self.found {
            Some(found) => found.ids.get(product).copied(),
            None => self
                .pipeline
                .products
                .iter()
                .position(|declared| declared.name == product)
                .map(ProductId::at),
        }
    }

    /// `product`'s declaration.
    pub(crate) fn product(&self, product: &str) -> Option<&'p ProductDef> {
        match &self.found {
            Some(found) => found
                .ids
                .get(product)
                .map(|id| &self.pipeline.products[id.index()]),
            None => self
                .pipeline
                .products
                .iter()
                .find(|declared| declared.name == product),
        }
    }

    /// The operation called `name`.
    pub(crate) fn operation(&self, name: &str) -> Option<&'p OperationDef> {
        match &self.found {
            Some(found) => found.operations.get(name).copied(),
            None => self
                .pipeline
                .operations
                .iter()
                .find(|operation| operation.name == name),
        }
    }

    /// The step that makes `product`, and the position of its output.
    pub(crate) fn producer(&self, product: &str) -> Option<(&'p Invocation, usize)> {
        match &self.found {
            Some(found) => found
                .ids
                .get(product)
                .and_then(|id| found.producers[id.index()]),
            None => self.pipeline.invocations.iter().find_map(|invocation| {
                let index = invocation
                    .outputs
                    .iter()
                    .position(|output| output == product)?;
                Some((invocation, index))
            }),
        }
    }

    /// See [`Pipeline::is_source`].
    pub(crate) fn is_source(&self, product: &str) -> bool {
        self.product(product).is_some() && self.producer(product).is_none()
    }

    /// See [`Pipeline::is_folder`].
    pub(crate) fn is_folder(&self, product: &str) -> bool {
        match self.output_port(product) {
            Some((_, _, port)) => port.folder,
            None => self
                .product(product)
                .is_some_and(|declared| declared.folder),
        }
    }

    /// See [`Pipeline::stage_of`].
    pub(crate) fn stage_of(&self, product: &str) -> Option<&'p str> {
        self.producer(product)
            .and_then(|(invocation, _)| invocation.stage.as_deref())
    }

    /// See [`Pipeline::path_template_for`].
    pub(crate) fn path_template_for(&self, product: &str) -> Option<Cow<'p, PathTemplate>> {
        if let Some((sibling, sibling_extension, suffix)) = self.beside(product) {
            // The sibling's own file: `{@product}` is its name, not this one's.
            let template = self.path_template_for(sibling)?.with_product(sibling);
            let stem = template
                .without_extension(sibling_extension)
                .unwrap_or(template);
            return Some(Cow::Owned(stem.with_extension(suffix)));
        }
        let template = self.path_rule_for(product)?.resolve(self.holder(product));
        Some(match self.added_extension(product) {
            Some((extension, _)) => Cow::Owned(template.with_extension(extension)),
            None => template,
        })
    }

    /// Each product's path template, as [`PipelineIndex::path_template_for`]
    /// gives it, by [`ProductId`]: built once, for a caller that needs every
    /// product's template more than once.
    pub(crate) fn path_templates(&self) -> Vec<Option<Cow<'p, PathTemplate>>> {
        self.pipeline
            .products
            .iter()
            .map(|product| self.path_template_for(&product.name))
            .collect()
    }

    /// Whether `product` has a path template, without building it.
    pub(crate) fn has_path(&self, product: &str) -> bool {
        match self.beside(product) {
            Some((sibling, _, _)) => self.has_path(sibling),
            None => self.path_rule_for(product).is_some(),
        }
    }

    /// What `product` gives a path template's placeholders: its dimensions
    /// and whether a stage makes it.
    pub(crate) fn holder(&self, product: &str) -> Holder<'p> {
        Holder {
            entities_format: self.pipeline.entities_format.as_ref(),
            dimensions: self
                .product(product)
                .map_or(&[][..], |declared| declared.dimensions.as_slice()),
            in_stage: self.stage_of(product).is_some(),
        }
    }

    /// See [`Pipeline::path_rule_for`].
    pub(crate) fn path_rule_for(&self, product: &str) -> Option<&'p PathTemplate> {
        self.path_origin(product).map(|(_, template)| template)
    }

    /// The path rule `product` uses, as written, and where it comes from.
    /// An output written beside another has no rule of its own, whatever
    /// this finds; ask [`PipelineIndex::beside`] first.
    ///
    /// A default that needs `{@stage}` is for products made in a stage, so
    /// it does not find a source, which is left for a rule of its own or
    /// the recipe's default.
    pub(crate) fn path_origin(&self, product: &str) -> Option<(PathOrigin<'p>, &'p PathTemplate)> {
        let pipeline = self.pipeline;
        if let Some(template) = pipeline.product_paths.get(product) {
            return Some((PathOrigin::Explicit, template));
        }
        if let Some((stage, template)) = self.stage_path_rule(product) {
            return Some((PathOrigin::Stage(stage), template));
        }
        match &pipeline.path_template {
            Some(default) => (!(default.needs_stage() && self.is_source(product)))
                .then_some((PathOrigin::Default, default)),
            None => self
                .producer(product)
                .map(|_| (PathOrigin::BuiltIn, PathTemplate::built_in_output())),
        }
    }

    /// The step that makes `product`, the operation it calls, and the port
    /// that writes it.
    fn output_port(
        &self,
        product: &str,
    ) -> Option<(&'p Invocation, &'p OperationDef, &'p OutputPort)> {
        let (invocation, index) = self.producer(product)?;
        let operation = self.operation(&invocation.operation)?;
        Some((invocation, operation, operation.outputs.get(index)?))
    }

    /// See [`Pipeline::output_extension`].
    pub(crate) fn output_extension(&self, product: &str) -> Option<(&'p str, &'p str)> {
        let (_, operation, port) = self.output_port(product)?;
        if port.beside.is_some() {
            return None;
        }
        Some((operation.name.as_str(), port.extension.as_deref()?))
    }

    /// See [`Pipeline::beside`].
    pub(crate) fn beside(&self, product: &str) -> Option<(&'p str, &'p str, &'p str)> {
        if self.is_source(product) {
            let beside = self.product(product)?.beside.as_ref()?;
            let sibling = self.product(&beside.sibling)?;
            let extension = sibling.extension.as_deref()?;
            return Some((sibling.name.as_str(), extension, beside.suffix.as_str()));
        }
        let (invocation, operation, port) = self.output_port(product)?;
        let beside = port.beside.as_ref()?;
        let index = operation
            .outputs
            .iter()
            .position(|output| output.name == beside.sibling)?;
        let sibling = invocation.outputs.get(index)?;
        let extension = operation.outputs[index].extension.as_deref()?;
        Some((sibling.as_str(), extension, beside.suffix.as_str()))
    }

    /// See [`Pipeline::default_extension`].
    pub(crate) fn default_extension(&self, product: &str) -> Option<(&'p str, ExtensionSource)> {
        let pipeline = self.pipeline;
        let staged = self.stage_of(product).and_then(|stage| {
            stage_and_parents(stage).find_map(|name| {
                let stage = pipeline
                    .stages
                    .iter()
                    .find(|candidate| candidate.name == name)?;
                Some((
                    stage.extension.as_deref()?,
                    ExtensionSource::Stage(stage.name.clone()),
                ))
            })
        });
        staged.or_else(|| Some((pipeline.extension.as_deref()?, ExtensionSource::Default)))
    }

    /// See [`Pipeline::expected_extension`].
    pub(crate) fn expected_extension(&self, product: &str) -> Option<(&'p str, ExtensionSource)> {
        if self.beside(product).is_some() {
            return None;
        }
        if let Some((operation, extension)) = self.output_extension(product) {
            return Some((extension, ExtensionSource::Operation(operation.to_owned())));
        }
        if let Some(declared) = self.product(product) {
            if let Some(extension) = declared.extension.as_deref() {
                return Some((extension, ExtensionSource::Source(declared.name.clone())));
            }
        }
        self.rule_extension(product)
    }

    /// The `ext:` default, for a product whose path is a default rule. A
    /// folder takes only the extension it declares.
    fn rule_extension(&self, product: &str) -> Option<(&'p str, ExtensionSource)> {
        if self.pipeline.product_paths.contains_key(product) {
            return None;
        }
        let default = self.default_extension(product)?;
        (!self.is_folder(product)).then_some(default)
    }

    /// See [`Pipeline::added_extension`].
    pub(crate) fn added_extension(&self, product: &str) -> Option<(&'p str, ExtensionSource)> {
        let expected = self.expected_extension(product)?;
        self.path_rule_for(product)?
            .extension()
            .is_none()
            .then_some(expected)
    }

    /// See [`Pipeline::stage_path_rule`].
    pub(crate) fn stage_path_rule(&self, product: &str) -> Option<(&'p str, &'p PathTemplate)> {
        let pipeline = self.pipeline;
        let stage = self.stage_of(product)?;
        stage_and_parents(stage).find_map(|name| {
            pipeline
                .stages
                .iter()
                .find(|candidate| candidate.name == name)
                .and_then(|stage| Some((stage.name.as_str(), stage.path_template.as_ref()?)))
        })
    }
}
