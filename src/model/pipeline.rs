use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{DefinitionSubject, ResolveError};
use crate::template::{EntitiesFormat, PathTemplate};

use super::{
    ArtifactInstance, BesideFile, Call, CallId, CheckDef, CommandDef, DefaultChecks,
    DefaultReviews, EntityBinding, ExtensionSource, Invocation, OperationDef, PipelineIndex,
    ProductDef, Removal, ResourcesDef, ReviewDef, SidecarGroup, SourceInventory, SourceRecord,
    StageDef, ToolList, UsesDef,
};

#[derive(Clone, Debug, Default)]
pub struct Pipeline {
    pub root: Option<(PathBuf, usize)>,
    pub output: Option<(PathBuf, usize)>,
    pub products: Vec<ProductDef>,
    pub operations: Vec<OperationDef>,
    pub invocations: Vec<Invocation>,
    pub commands: Vec<CommandDef>,
    pub resources: Vec<ResourcesDef>,
    pub tools: Vec<String>,
    pub uses: Vec<UsesDef>,
    pub default_uses: Option<ToolList>,
    pub checks: Vec<CheckDef>,
    pub default_checks: DefaultChecks,
    pub reviews: Vec<ReviewDef>,
    pub default_reviews: DefaultReviews,
    pub path_template: Option<PathTemplate>,
    pub entities_format: Option<EntitiesFormat>,
    pub extension: Option<String>,
    pub product_paths: BTreeMap<String, PathTemplate>,
    pub stages: Vec<StageDef>,
    pub sidecar_groups: Vec<SidecarGroup>,
    pub calls: Vec<Call>,
    pub files: Vec<SourceFile>,
    pub folder: Option<String>,
    pub types: TypeChecking,
    pub type_decls: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TypeChecking {
    #[default]
    Standard,
    Strict,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceFile {
    pub path: String,
    pub blob: String,
}

impl Pipeline {
    pub fn written_call(&self, call: CallId) -> &Call {
        &self.calls[self.written_call_id(call).index()]
    }

    pub fn written_call_id(&self, mut call: CallId) -> CallId {
        while let Some(parent) = self.calls[call.index()].parent {
            call = parent;
        }
        call
    }

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
    pub fn dimension_order(&self) -> Vec<String> {
        let mut seen = FxHashSet::default();
        self.products
            .iter()
            .flat_map(|product| &product.dimensions)
            .filter(|dimension| seen.insert(dimension.as_str()))
            .cloned()
            .collect()
    }

    pub fn stage_of(&self, product: &str) -> Option<&str> {
        PipelineIndex::scan(self).stage_of(product)
    }

    pub fn path_template_for(&self, product: &str) -> Option<Cow<'_, PathTemplate>> {
        PipelineIndex::scan(self).path_template_for(product)
    }

    pub fn path_rule_for(&self, product: &str) -> Option<&PathTemplate> {
        PipelineIndex::scan(self).path_rule_for(product)
    }

    pub fn output_extension(&self, product: &str) -> Option<(&str, &str)> {
        PipelineIndex::scan(self).output_extension(product)
    }

    pub fn beside(&self, product: &str) -> Option<BesideFile<'_>> {
        PipelineIndex::scan(self).beside(product)
    }

    pub fn default_extension(&self, product: &str) -> Option<(&str, ExtensionSource)> {
        PipelineIndex::scan(self).default_extension(product)
    }

    pub fn expected_extension(&self, product: &str) -> Option<(&str, ExtensionSource)> {
        PipelineIndex::scan(self).expected_extension(product)
    }

    pub fn added_extension(&self, product: &str) -> Option<(&str, ExtensionSource)> {
        PipelineIndex::scan(self).added_extension(product)
    }

    pub fn stage_path_rule(&self, product: &str) -> Option<(&str, &PathTemplate)> {
        PipelineIndex::scan(self).stage_path_rule(product)
    }

    pub fn stage_path_template(&self, product: &str) -> Option<&PathTemplate> {
        self.stage_path_rule(product).map(|(_, template)| template)
    }

    pub fn sidecar_group(&self, name: &str) -> Option<&SidecarGroup> {
        self.sidecar_groups.iter().find(|group| group.name == name)
    }

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

    pub fn is_folder(&self, product: &str) -> bool {
        PipelineIndex::scan(self).is_folder(product)
    }

    pub fn is_source(&self, product: &str) -> bool {
        PipelineIndex::scan(self).is_source(product)
    }

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

    pub(crate) fn check_sources<'a>(
        &'a self,
        inventory: &'a SourceInventory,
        mut add: impl FnMut(&'a ProductDef, &'a SourceRecord) -> bool,
    ) -> Result<(), ResolveError> {
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

pub struct DimensionOrders<'p> {
    products: FxHashMap<&'p str, &'p [String]>,
    pipeline: Vec<String>,
}

impl<'p> DimensionOrders<'p> {
    pub fn new(pipeline: &'p Pipeline) -> Self {
        let mut products = FxHashMap::default();
        for product in &pipeline.products {
            products
                .entry(product.name.as_str())
                .or_insert(product.dimensions.as_slice());
        }
        Self {
            products,
            pipeline: pipeline.dimension_order(),
        }
    }

    pub fn of(&self, removal: &Removal) -> &[String] {
        removal
            .product
            .as_deref()
            .and_then(|name| self.products.get(name).copied())
            .unwrap_or(&self.pipeline)
    }
}
