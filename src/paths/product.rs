//! The path a product's template gives each of its artifacts, with the
//! template and stage found once per product.

use std::borrow::Cow;

use crate::model::{Artifact, Pipeline, PipelineIndex};

use super::template::{
    error, product_text, push_escaped, PathError, PathPart, PathPlaceholder, PathTemplate,
};

use super::components::{push_encoded, unusable_path};

/// Bind `artifact` to its relative path by its product's path template,
/// `template`. `label` names the path in errors: a
/// product's rule, or an artifact.
pub(crate) fn bind_path(
    index: &PipelineIndex<'_>,
    template: &PathTemplate,
    artifact: Artifact<'_>,
    label: impl Fn() -> String,
) -> Result<String, PathError> {
    ProductPath::with_template(index, artifact.product, Cow::Borrowed(template))
        .bind(artifact, label)
}

/// Binds many artifacts' paths, as [`bind_path`] does, finding each
/// product's template and stage once.
pub(crate) struct PathBinder<'p> {
    index: PipelineIndex<'p>,
    /// Each pipeline product's, by its id, once it is needed.
    products: Vec<Option<ProductPath<'p>>>,
    /// The same, by the product's number in a DAG's artifact table.
    numbered: Vec<Option<ProductPath<'p>>>,
}

impl<'p> PathBinder<'p> {
    pub(crate) fn new(pipeline: &'p Pipeline) -> Self {
        Self {
            index: PipelineIndex::new(pipeline),
            products: std::iter::repeat_with(|| None)
                .take(pipeline.products.len())
                .collect(),
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
        product.bind(artifact, label)
    }

    pub(crate) fn bind(
        &mut self,
        artifact: Artifact<'_>,
        label: impl Fn() -> String,
    ) -> Result<String, PathError> {
        let Some(id) = self.index.id(artifact.product) else {
            // Not one of the pipeline's products: nothing shares its path.
            let product = ProductPath::new(&self.index, artifact.product)?;
            return product.bind(artifact, label);
        };
        let product = match &mut self.products[id.index()] {
            Some(product) => product,
            slot => slot.insert(ProductPath::new(&self.index, artifact.product)?),
        };
        product.bind(artifact, label)
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
        Ok(Self::with_template(index, product, template))
    }

    /// `product`'s, with `template` already built.
    fn with_template(
        index: &PipelineIndex<'p>,
        product: &str,
        template: Cow<'p, PathTemplate>,
    ) -> Self {
        Self {
            template,
            name: product_text(product).into_owned(),
            stage: stage_directories(index, product),
        }
    }

    fn bind(
        &self,
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
                            "path template for `{}` uses `{}`, but `{}` is not made in a stage; write `[{}/]` to leave the stage's directory out for products outside every stage",
                            artifact.product,
                            PathPlaceholder::Stage,
                            artifact.product,
                            PathPlaceholder::Stage
                        ))
                        .focus(PathPlaceholder::Stage.to_string())
                    })?;
                    relative.push_str(stage);
                }
                // A pipeline with an `entities:` line has `{@entities}`
                // written out; the path checks fail a rule without one first.
                PathPart::Placeholder(PathPlaceholder::Entities) => {
                    return Err(error(format!(
                        "path template for `{}` uses `{}`, but the pipeline has no `entities:` line to spell it",
                        artifact.product,
                        PathPlaceholder::Entities
                    ))
                    .focus(PathPlaceholder::Entities.to_string()));
                }
                PathPart::Placeholder(placeholder @ PathPlaceholder::Dimension(dimension, _)) => {
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
                PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
            }
        }
        if let Some(reason) = unusable_path(&relative) {
            return Err(error(format!("{} {reason}: `{relative}`", label())));
        }
        Ok(relative)
    }
}

/// The directories `{@stage}` gives `product`'s paths, each nested stage
/// one, or `None` for a product made in no stage.
pub(crate) fn stage_directories(index: &PipelineIndex<'_>, product: &str) -> Option<String> {
    index.stage_of(product).map(|stage| {
        let mut directories = String::new();
        for (index, component) in stage.split('/').enumerate() {
            if index > 0 {
                directories.push('/');
            }
            push_encoded(&mut directories, component);
        }
        directories
    })
}

/// `product`'s path template, `template`, with `{@product}` and `{@stage}`
/// written out, as every artifact of it shares them, for showing beside its
/// declaration: `derivatives/yield_table/{@entities}.csv`.
pub(crate) fn shown_path(
    index: &PipelineIndex<'_>,
    product: &str,
    template: &PathTemplate,
) -> String {
    let path = ProductPath::with_template(index, product, Cow::Borrowed(template));
    let mut shown = String::new();
    for part in path.template.parts() {
        match part {
            PathPart::Literal(value) => push_escaped(&mut shown, value),
            PathPart::Placeholder(PathPlaceholder::Product) => shown.push_str(&path.name),
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                shown.push_str(path.stage.as_deref().unwrap_or("{@stage}"));
            }
            PathPart::Placeholder(placeholder) => shown.push_str(&placeholder.to_string()),
            PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
        }
    }
    shown
}
