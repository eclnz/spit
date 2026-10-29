//! An index over a pipeline, which finds each product's step once.

use std::borrow::Cow;

use rustc_hash::FxHashMap;

use crate::paths::{stage_directories, EntitiesFormat, Holder, PathTemplate};

use super::{
    stage_and_parents, BesideFile, ExtensionSource, Invocation, OperationDef, OutputPort, Pipeline,
    ProductDef,
};

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
/// a single question. Either borrows the pipeline, so the pipeline cannot
/// change while an index of it is held.
pub(crate) struct PipelineIndex<'p> {
    pub(crate) pipeline: &'p Pipeline,
    found: Option<Found<'p>>,
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
        let Some(first) = self.beside(product) else {
            let template = self.path_rule_for(product)?.resolve(self.holder(product));
            return Some(match self.added_extension(product) {
                Some((extension, _)) => Cow::Owned(template.with_extension(extension)),
                None => template,
            });
        };
        // A file beside a file beside another follows it in turn, to the
        // first with a rule of its own; a loop, as a chain of steps may be
        // long.
        let mut chain = vec![(product, first)];
        while let Some(beside) = self.beside(chain[chain.len() - 1].1.sibling) {
            if chain.len() > self.pipeline.products.len() {
                return None;
            }
            chain.push((chain[chain.len() - 1].1.sibling, beside));
        }
        let root = chain[chain.len() - 1].1.sibling;
        let mut template = self.path_template_for(root)?.into_owned();
        for &(follower, beside) in chain.iter().rev() {
            // The sibling's own file: `{@product}` is its name, `{@stage}`
            // its stage, and a value the step pins is the one it reads.
            let pinned = beside
                .input
                .and_then(|input| self.producer(follower)?.0.inputs.get(input))
                .map(|binding| &binding.pinned);
            let stage = (self.stage_of(beside.sibling) != self.stage_of(follower))
                .then(|| stage_directories(self, beside.sibling))
                .flatten();
            template = template.written_out(beside.sibling, stage.as_deref(), pinned);
            if !beside.append {
                let replaced = beside.replaces?;
                template = template.without_extension(replaced).unwrap_or(template);
            }
            template = template.with_extension(beside.suffix);
        }
        Some(Cow::Owned(template))
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
        let mut current = product;
        for _ in 0..=self.pipeline.products.len() {
            match self.beside(current) {
                Some(beside) => current = beside.sibling,
                None => return self.path_rule_for(current).is_some(),
            }
        }
        false
    }

    /// What `product` gives a path template's placeholders: its dimensions
    /// and whether a stage makes it.
    pub(crate) fn holder(&self, product: &str) -> Holder<'p> {
        Holder {
            entities_format: self.entities_format_for(product),
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
    /// A default that needs `{@stage}` is for products made in a stage, and
    /// one that needs `{@entities}` for products with dimensions, so neither
    /// finds a source it cannot write, which is left for a rule of its own
    /// or the recipe's default.
    pub(crate) fn path_origin(&self, product: &str) -> Option<(PathOrigin<'p>, &'p PathTemplate)> {
        let pipeline = self.pipeline;
        if let Some(template) = pipeline.product_paths.get(product) {
            return Some((PathOrigin::Explicit, template));
        }
        if let Some((stage, template)) = self.stage_path_rule(product) {
            return Some((PathOrigin::Stage(stage), template));
        }
        let default = pipeline.path_template.as_ref()?;
        // A default a source cannot use is left for the source's own rule or
        // the recipe's default, as no source is made in a stage.
        let unusable = default.needs_stage()
            || (default.needs_entities()
                && self
                    .product(product)
                    .is_some_and(|declared| declared.dimensions.is_empty()));
        (!(unusable && self.is_source(product))).then_some((PathOrigin::Default, default))
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
    pub(crate) fn beside(&self, product: &str) -> Option<BesideFile<'p>> {
        if self.is_source(product) {
            let beside = self.product(product)?.beside.as_ref()?;
            let sibling = self.product(&beside.sibling)?;
            return Some(BesideFile {
                sibling: sibling.name.as_str(),
                replaces: sibling.extension.as_deref().filter(|_| !beside.append),
                suffix: beside.suffix.as_str(),
                append: beside.append,
                input: None,
            });
        }
        let (invocation, operation, port) = self.output_port(product)?;
        let beside = port.beside.as_ref()?;
        let (sibling, replaces, input) = match operation
            .outputs
            .iter()
            .position(|output| output.name == beside.sibling)
        {
            Some(index) => (
                invocation.outputs.get(index)?.as_str(),
                operation.outputs[index].extension.as_deref(),
                None,
            ),
            None => {
                let index = operation
                    .inputs
                    .iter()
                    .position(|input| input.name == beside.sibling)?;
                let sibling = invocation.inputs.get(index)?.product.as_str();
                let replaces = if beside.append {
                    None
                } else {
                    self.file_extension(sibling)
                };
                (sibling, replaces, Some(index))
            }
        };
        Some(BesideFile {
            sibling,
            replaces: replaces.filter(|_| !beside.append),
            suffix: beside.suffix.as_str(),
            append: beside.append,
            input,
        })
    }

    /// The extension `product`'s file has, as far as the pipeline says: the
    /// one a file written beside another takes from its suffix, else the
    /// one it must have, else the one its rule ends with.
    fn file_extension(&self, product: &str) -> Option<&'p str> {
        if let Some((_, _, port)) = self.output_port(product) {
            if port.beside.is_some() {
                return port.extension.as_deref();
            }
        } else if let Some(declared) = self
            .product(product)
            .filter(|declared| declared.beside.is_some())
        {
            return declared.extension.as_deref();
        }
        match self.expected_extension(product) {
            Some((extension, _)) => Some(extension),
            None => self.path_rule_for(product)?.extension(),
        }
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

    /// How `{@entities}` is spelled for `product`: by its stage's `default
    /// entities:`, or the nearest stage around it with one, else the
    /// pipeline's.
    pub(crate) fn entities_format_for(&self, product: &str) -> Option<&'p EntitiesFormat> {
        let pipeline = self.pipeline;
        self.stage_of(product)
            .and_then(|stage| {
                stage_and_parents(stage).find_map(|name| {
                    pipeline
                        .stages
                        .iter()
                        .find(|candidate| candidate.name == name)
                        .and_then(|stage| stage.entities_format.as_ref())
                })
            })
            .or(pipeline.entities_format.as_ref())
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
