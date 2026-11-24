use crate::model::{
    stage_directories, Artifact, EntityBinding, PathOrigin, Pipeline, PipelineIndex,
};
use crate::template::{
    decode_component, error, product_text, push_escaped, Gap, PathError, PathPart, PathPattern,
    PathPlaceholder, PathTemplate, RenderError,
};

use super::components::unusable_path;

pub(crate) fn bind_path(
    index: &PipelineIndex<'_>,
    template: &PathTemplate,
    artifact: Artifact<'_>,
    label: impl Fn() -> String,
) -> Result<String, PathError> {
    ProductPath::with_template(index, artifact.product, template).bind(artifact, label)
}

pub(crate) struct PathBinder<'p> {
    index: PipelineIndex<'p>,
    products: Vec<Option<ProductPath>>,
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

    pub(crate) fn index(&self) -> &PipelineIndex<'p> {
        &self.index
    }

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

    pub(crate) fn read_back(&mut self, product: &str, path: &str) -> Option<EntityBinding> {
        let id = self.index.id(product)?;
        let product = match &mut self.products[id.index()] {
            Some(found) => found,
            slot => slot.insert(ProductPath::new(&self.index, product).ok()?),
        };
        if !product.writes {
            return None;
        }
        product
            .pattern
            .match_path(path)?
            .into_iter()
            .map(|(dimension, value)| Some((dimension, decode_component(value)?)))
            .collect::<Option<EntityBinding>>()
    }

    pub(crate) fn bind(
        &mut self,
        artifact: Artifact<'_>,
        label: impl Fn() -> String,
    ) -> Result<String, PathError> {
        let Some(id) = self.index.id(artifact.product) else {
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

struct ProductPath {
    pattern: PathPattern,
    writes: bool,
    shared: bool,
    here: Option<String>,
}

impl ProductPath {
    fn new(index: &PipelineIndex<'_>, product: &str) -> Result<Self, PathError> {
        let template = index
            .path_template_for(product)
            .ok_or_else(|| error(format!("no path template for product `{product}`")))?;
        Ok(Self::with_template(index, product, &template))
    }

    fn with_template(index: &PipelineIndex<'_>, product: &str, template: &PathTemplate) -> Self {
        let stage = stage_directories(index, product);
        let shared = !matches!(
            index.path_origin(product),
            Some((PathOrigin::Explicit, _)) | None
        ) && index.beside(product).is_none();
        let here = index
            .product(product)
            .and_then(|product| product.here.clone())
            .filter(|folder| !folder.is_empty());
        Self {
            pattern: PathPattern::new(template, product, stage.as_deref()),
            writes: template.writes_paths(),
            shared,
            here,
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
                RenderError::Gap(Gap::Entities) => error(unspelled_entities(
                    artifact.product,
                    artifact.entities.is_empty(),
                ))
                .focus(PathPlaceholder::Entities.to_string()),
                RenderError::Reads => error(format!(
                    "path rule for `{}` has `*` or a fixed value, so it reads paths and writes none; only a source's records, which keep the path discovery found, can give one",
                    artifact.product
                )),
                RenderError::Absent(placeholder) => {
                    let dimension = placeholder.name();
                    let product = artifact.product;
                    let fix = PathPlaceholder::hint(dimension).unwrap_or_else(|| {
                        if self.shared {
                            format!("; put it in `[...]` if only some products have it, or give `{product}` a rule of its own with `path {product}:`")
                        } else {
                            "; put it in `[...]` if only some products have it".to_owned()
                        }
                    });
                    error(format!(
                        "path template for `{product}` uses absent dimension `{dimension}`{fix}"
                    ))
                    .focus(placeholder.to_string())
                }
            })?;
        if let Some(reason) = unusable_path(&relative) {
            return Err(error(format!("{} {reason}: `{relative}`", label())));
        }
        match &self.here {
            Some(folder) if folder == ".." || folder.starts_with("../") => Err(error(format!(
                "{} is `{folder}/{relative}`, outside the pipeline's folder; a `here` source is kept in the pipeline's folder or beneath it",
                label()
            ))),
            Some(folder) => Ok(format!("{folder}/{relative}")),
            None => Ok(relative),
        }
    }
}

pub(crate) fn shown_paths(
    index: &PipelineIndex<'_>,
    product: &str,
    template: &PathTemplate,
) -> Vec<String> {
    let stage = stage_directories(index, product);
    template
        .rules()
        .map(|rule| shown_rule(product, rule, stage.as_deref()))
        .collect()
}

fn shown_rule(product: &str, rule: &PathTemplate, stage: Option<&str>) -> String {
    let mut shown = String::new();
    for part in rule.parts() {
        match part {
            PathPart::Literal(value) => push_escaped(&mut shown, value),
            PathPart::Placeholder(PathPlaceholder::Product) => {
                shown.push_str(&product_text(product));
            }
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                shown.push_str(stage.unwrap_or("{@stage}"));
            }
            PathPart::Placeholder(placeholder) => shown.push_str(&placeholder.to_string()),
            PathPart::Wildcard => shown.push('*'),
            PathPart::Fixed { dimension, value } => {
                shown.push_str(&format!("{{{dimension}={value}}}"));
            }
            PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
        }
    }
    shown
}

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

pub(crate) fn screen_record_path(index: &PipelineIndex<'_>, product: &str) -> String {
    let name = product_text(product);
    let fallback = format!("{name}.screens.csv");
    let Some(rule) = index
        .path_rule_for(product)
        .filter(|_| !index.is_source(product))
    else {
        return fallback;
    };
    let stage = stage_directories(index, product);
    let varies = |part: &PathPart| {
        matches!(
            part,
            PathPart::Placeholder(PathPlaceholder::Dimension(..) | PathPlaceholder::Entities)
        )
    };
    let mut path = String::new();
    let mut parts: Vec<_> = rule.parts().iter().rev().collect();
    while let Some(part) = parts.pop() {
        match part {
            PathPart::Literal(text) => path.push_str(text),
            PathPart::Placeholder(PathPlaceholder::Product) => path.push_str(&name),
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                path.push_str(stage.as_deref().unwrap_or(""))
            }
            PathPart::Group(inner)
                if !inner.iter().any(varies)
                    && (stage.is_some()
                        || !inner.iter().any(|part| {
                            matches!(part, PathPart::Placeholder(PathPlaceholder::Stage))
                        })) =>
            {
                parts.extend(inner.iter().rev());
            }
            PathPart::Group(_) => {}
            _ => {
                return path
                    .rsplit_once('/')
                    .map_or(fallback.clone(), |(folder, _)| {
                        format!("{folder}/{fallback}")
                    })
            }
        }
    }
    if path.ends_with('/') {
        return format!("{path}{fallback}");
    }
    if let Some(extension) = index.file_extension(product) {
        if let Some(stem) = path.strip_suffix(extension) {
            return format!("{stem}.screens.csv");
        }
    }
    format!("{path}.screens.csv")
}
