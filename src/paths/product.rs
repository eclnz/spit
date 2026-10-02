//! The path a product's template gives each of its artifacts, with the
//! template and stage found once per product.

use std::borrow::Cow;

use rustc_hash::FxHashMap;

use crate::model::{Artifact, Pipeline, PipelineIndex};

use super::template::{error, PathError, PathPart, PathPlaceholder, PathTemplate};

use super::components::{push_encoded, unusable_path};

/// Bind `artifact` to its relative path. `dimensions` gives the product's
/// declared dimension order, which `{@entities}` follows. `label` names the
/// path in errors: a product's rule, or an artifact.
pub(crate) fn bind_path(
    index: &PipelineIndex<'_>,
    dimensions: &[String],
    artifact: Artifact<'_>,
    label: impl Fn() -> String,
) -> Result<String, PathError> {
    ProductPath::new(index, artifact.product)?.bind(dimensions, artifact, label)
}

/// Binds many artifacts' paths, as [`bind_path`] does, finding each
/// product's template and stage once.
pub(crate) struct PathBinder<'p> {
    index: PipelineIndex<'p>,
    products: FxHashMap<String, ProductPath<'p>>,
    /// The same, by the product's number in a DAG's artifact table.
    numbered: Vec<Option<ProductPath<'p>>>,
}

impl<'p> PathBinder<'p> {
    pub(crate) fn new(pipeline: &'p Pipeline) -> Self {
        Self {
            index: PipelineIndex::new(pipeline),
            products: FxHashMap::default(),
            numbered: Vec::new(),
        }
    }

    /// The pipeline whose paths this binds.
    pub(crate) fn index(&self) -> &PipelineIndex<'p> {
        &self.index
    }

    /// As [`PathBinder::bind`], for an artifact whose product has `number`
    /// in its DAG's artifact table.
    pub(crate) fn bind_numbered(
        &mut self,
        number: u32,
        dimensions: &[String],
        artifact: Artifact<'_>,
        label: impl Fn() -> String,
    ) -> Result<String, PathError> {
        let number = number as usize;
        if self.numbered.len() <= number {
            self.numbered.resize_with(number + 1, || None);
        }
        let product = match &mut self.numbered[number] {
            Some(product) => product,
            slot => slot.insert(ProductPath::new(&self.index, artifact.product)?),
        };
        product.bind(dimensions, artifact, label)
    }

    pub(crate) fn bind(
        &mut self,
        dimensions: &[String],
        artifact: Artifact<'_>,
        label: impl Fn() -> String,
    ) -> Result<String, PathError> {
        let product = match self.products.get(artifact.product) {
            Some(product) => product,
            None => {
                let product = ProductPath::new(&self.index, artifact.product)?;
                self.products
                    .entry(artifact.product.to_owned())
                    .or_insert(product)
            }
        };
        product.bind(dimensions, artifact, label)
    }
}

/// What every path of one product shares: its template, its name as a
/// path gives it, and its stage's directories.
struct ProductPath<'p> {
    template: Cow<'p, PathTemplate>,
    /// `alias::name` would put colons in file names.
    name: String,
    /// Each nested stage is a directory.
    stage: Option<String>,
}

impl<'p> ProductPath<'p> {
    fn new(index: &PipelineIndex<'p>, product: &str) -> Result<Self, PathError> {
        let template = index
            .path_template_for(product)
            .ok_or_else(|| error(format!("no path template for product `{product}`")))?;
        let stage = index.stage_of(product).map(|stage| {
            let mut directories = String::new();
            for (index, component) in stage.split('/').enumerate() {
                if index > 0 {
                    directories.push('/');
                }
                push_encoded(&mut directories, component);
            }
            directories
        });
        Ok(Self {
            template,
            name: product.replace("::", "."),
            stage,
        })
    }

    fn bind(
        &self,
        dimensions: &[String],
        artifact: Artifact<'_>,
        label: impl Fn() -> String,
    ) -> Result<String, PathError> {
        let mut relative = String::with_capacity(self.template.text.len() + 32);
        for part in self.template.parts() {
            match part {
                PathPart::Literal(value) => relative.push_str(value),
                PathPart::Placeholder(PathPlaceholder::Product) => relative.push_str(&self.name),
                PathPart::Placeholder(PathPlaceholder::Stage) => {
                    let stage = self.stage.as_deref().ok_or_else(|| {
                        error(format!(
                            "path template for `{}` uses `{}`, but `{}` is not made in a stage",
                            artifact.product,
                            PathPlaceholder::Stage,
                            artifact.product
                        ))
                        .focus(PathPlaceholder::Stage.to_string())
                    })?;
                    relative.push_str(stage);
                }
                PathPart::Placeholder(PathPlaceholder::Entities) => {
                    push_entities(&mut relative, artifact, dimensions)?;
                }
                PathPart::Placeholder(placeholder @ PathPlaceholder::Dimension(dimension)) => {
                    let value = artifact.entities.get(dimension).ok_or_else(|| {
                        error(format!(
                            "path template for `{}` uses absent dimension `{dimension}`{}",
                            artifact.product,
                            PathPlaceholder::hint(dimension).unwrap_or_else(|| {
                                "; put it in `[...]` if only some products have it".to_owned()
                            })
                        ))
                        .focus(placeholder.to_string())
                    })?;
                    push_encoded(&mut relative, value);
                }
                // A product with dimensions has `{@labels}` written out.
                PathPart::Placeholder(PathPlaceholder::Labels) => {
                    return Err(error(format!(
                        "path template for `{}` uses `{}`, but `{}` has no dimensions; put it in `[...]`, as `[{}_]`",
                        artifact.product,
                        PathPlaceholder::Labels,
                        artifact.product,
                        PathPlaceholder::Labels
                    ))
                    .focus(PathPlaceholder::Labels.to_string()));
                }
                PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
            }
        }
        if let Some(reason) = unusable_path(&relative) {
            return Err(error(format!("{} {reason}: `{relative}`", label())));
        }
        Ok(relative)
    }
}

/// `product`'s path template with `{@product}` and `{@stage}` written out, as
/// every artifact of it shares them, for showing beside its declaration:
/// `derivatives/yield_table/{@entities}.csv`.
pub(crate) fn shown_path(index: &PipelineIndex<'_>, product: &str) -> Option<String> {
    let path = ProductPath::new(index, product).ok()?;
    let mut shown = String::new();
    for part in path.template.parts() {
        match part {
            PathPart::Literal(value) => shown.push_str(value),
            PathPart::Placeholder(PathPlaceholder::Product) => shown.push_str(&path.name),
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                shown.push_str(path.stage.as_deref().unwrap_or("{@stage}"));
            }
            PathPart::Placeholder(placeholder) => shown.push_str(&placeholder.to_string()),
            PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
        }
    }
    Some(shown)
}

/// Add what `{@entities}` binds to: each dimension as `dimension=value`, in
/// declared order and joined by `__`, or `global` for none.
fn push_entities(
    relative: &mut String,
    artifact: Artifact<'_>,
    dimensions: &[String],
) -> Result<(), PathError> {
    if dimensions.is_empty() {
        relative.push_str("global");
    }
    for (index, dimension) in dimensions.iter().enumerate() {
        let value = artifact.entities.get(dimension).ok_or_else(|| {
            error(format!(
                "artifact `{artifact}` lacks dimension `{dimension}`"
            ))
        })?;
        if index > 0 {
            relative.push_str("__");
        }
        push_encoded(relative, dimension);
        relative.push('=');
        push_encoded(relative, value);
    }
    Ok(())
}
