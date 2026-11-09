//! The path a product's template gives each of its artifacts, with its
//! [`PathPattern`] compiled once per product.

use crate::model::{stage_directories, Artifact, Pipeline, PipelineIndex};
use crate::template::{
    error, product_text, push_escaped, Gap, PathError, PathPart, PathPattern, PathPlaceholder,
    PathTemplate, RenderError,
};

use super::components::unusable_path;

/// Bind `artifact` to its relative path by its product's path template,
/// `template`. `label` names the path in errors: a
/// product's rule, or an artifact.
pub(crate) fn bind_path(
    index: &PipelineIndex<'_>,
    template: &PathTemplate,
    artifact: Artifact<'_>,
    label: impl Fn() -> String,
) -> Result<String, PathError> {
    ProductPath::with_template(index, artifact.product, template).bind(artifact, label)
}

/// Binds many artifacts' paths, as [`bind_path`] does, finding each
/// product's template and stage once.
pub(crate) struct PathBinder<'p> {
    index: PipelineIndex<'p>,
    /// Each pipeline product's, by its id, once it is needed.
    products: Vec<Option<ProductPath>>,
    /// The same, by the product's number in a DAG's artifact table.
    numbered: Vec<Option<ProductPath>>,
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

/// One product's [`PathPattern`], compiled once for all its artifacts.
struct ProductPath {
    pattern: PathPattern,
}

impl ProductPath {
    fn new(index: &PipelineIndex<'_>, product: &str) -> Result<Self, PathError> {
        let template = index
            .path_template_for(product)
            .ok_or_else(|| error(format!("no path template for product `{product}`")))?;
        Ok(Self::with_template(index, product, &template))
    }

    /// `product`'s, with `template` already built.
    fn with_template(index: &PipelineIndex<'_>, product: &str, template: &PathTemplate) -> Self {
        let stage = stage_directories(index, product);
        Self {
            pattern: PathPattern::new(template, product, stage.as_deref()),
        }
    }

    fn bind(
        &self,
        artifact: Artifact<'_>,
        label: impl Fn() -> String,
    ) -> Result<String, PathError> {
        let relative = self
            .pattern
            .render(|dimension| artifact.entities.get(dimension))
            .map_err(|problem| match problem {
                RenderError::Gap(Gap::Stage) => error(format!(
                    "path template for `{}` uses `{}`, but `{}` is not made in a stage; write `[{}/]` to leave the stage's directory out for products outside every stage",
                    artifact.product,
                    PathPlaceholder::Stage,
                    artifact.product,
                    PathPlaceholder::Stage
                ))
                .focus(PathPlaceholder::Stage.to_string()),
                // A product with dimensions and a spelling has `{@entities}`
                // written out.
                RenderError::Gap(Gap::Entities) => error(unspelled_entities(
                    artifact.product,
                    artifact.entities.is_empty(),
                ))
                .focus(PathPlaceholder::Entities.to_string()),
                RenderError::Absent(placeholder) => {
                    let dimension = placeholder.name();
                    error(format!(
                        "path template for `{}` uses absent dimension `{dimension}`{}",
                        artifact.product,
                        PathPlaceholder::hint(dimension).unwrap_or_else(|| {
                            "; put it in `[...]` if only some products have it".to_owned()
                        })
                    ))
                    .focus(placeholder.to_string())
                }
            })?;
        if let Some(reason) = unusable_path(&relative) {
            return Err(error(format!("{} {reason}: `{relative}`", label())));
        }
        Ok(relative)
    }
}

/// `product`'s path template, `template`, with `{@product}` and `{@stage}`
/// written out, as every artifact of it shares them, for showing beside its
/// declaration: `derivatives/yield_table/{@entities}.csv`.
pub(crate) fn shown_path(
    index: &PipelineIndex<'_>,
    product: &str,
    template: &PathTemplate,
) -> String {
    let stage = stage_directories(index, product);
    let mut shown = String::new();
    for part in template.parts() {
        match part {
            PathPart::Literal(value) => push_escaped(&mut shown, value),
            PathPart::Placeholder(PathPlaceholder::Product) => {
                shown.push_str(&product_text(product));
            }
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                shown.push_str(stage.as_deref().unwrap_or("{@stage}"));
            }
            PathPart::Placeholder(placeholder) => shown.push_str(&placeholder.to_string()),
            PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
        }
    }
    shown
}

/// Why `product`'s path keeps `{@entities}`: it has no dimensions, so no
/// `{@entities}`, as a product outside every stage has no `{@stage}`, or no
/// `default entities:` spells it. Keep in step with `push_resolved` in
/// `template/path.rs`, which writes it out otherwise.
pub(crate) fn unspelled_entities(product: &str, dimensionless: bool) -> String {
    let placeholder = PathPlaceholder::Entities;
    if dimensionless {
        format!(
            "path rule for `{product}` uses `{placeholder}`, but `{product}` has no dimensions; put it in `[...]`, as `[{placeholder}_]`"
        )
    } else {
        format!(
            "path rule for `{product}` uses `{placeholder}`, but no `default entities:` spells it; add one, as `default entities: {{@key}}={{@value}} separated \"__\"`"
        )
    }
}
