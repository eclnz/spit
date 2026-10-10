//! Checks of recipe declarations against their pipeline.

use super::*;

/// Diagnose a `.spitin` recipe at `path` without reading any data: its own
/// lines, then the pipeline its `pipeline` line names, then its rules and
/// any records against that pipeline. The pipeline's own errors are named
/// by file and line, since they are not in `text`.
pub fn diagnose_recipe(text: &str, path: &Path) -> Vec<Diagnostic> {
    let text = without_bom(text);
    let error = |message: String| Diagnostic::error(DiagnosticSource::Pipeline, None, message);
    let spec = match crate::inputs::parse_input_spec_at(text, path) {
        Ok(spec) => spec,
        Err(parse) => {
            let diagnostic = Diagnostic::located(DiagnosticSource::Pipeline, &parse, text);
            return finish(vec![diagnostic], text, None);
        }
    };
    let Some(pipeline_path) = &spec.pipeline else {
        let message =
            "name the pipeline this recipe is for, with a line such as `pipeline analysis.spit`";
        return finish(vec![error(message.to_owned())], text, None);
    };
    let shown = pipeline_path.display().to_string();
    let pipeline_text = match std::fs::read_to_string(pipeline_path) {
        Ok(pipeline_text) => pipeline_text,
        Err(reason) => {
            return finish(
                vec![error(format!("cannot read pipeline `{shown}`: {reason}"))],
                text,
                None,
            )
        }
    };
    match diagnose_checked(&pipeline_text, Context::at(pipeline_path)) {
        Ok(checked) => {
            let mut diagnostics = diagnose_recipe_against(text, &checked.pipeline);
            if spec
                .root_for(&checked.pipeline)
                .is_ok_and(|root| root.is_none())
            {
                diagnostics.push(error("a recipe names its dataset root with `root <directory>`, or inherits its pipeline's root".to_owned()));
            }
            // An inherited root's warning points at the pipeline, where it
            // is written, rather than at a recipe line with the same number.
            if spec.root.is_none() {
                for mut warning in checked.warnings {
                    if warning.message.starts_with("dataset root ") {
                        warning.file = Some(crate::imports::relative_path(
                            path.parent().unwrap_or_else(|| Path::new("")),
                            pipeline_path,
                        ));
                        warning.external_text = Some(Arc::from(pipeline_text.as_str()));
                        diagnostics.push(warning);
                    }
                }
            }
            // The rows of files `exclude from` lines name are read with the
            // recipe at its path, not with its text; each error says where.
            let rows = InputRules {
                exclusions: spec
                    .rules
                    .exclusions
                    .iter()
                    .filter(|rule| !rule.origin.starts_with("line "))
                    .cloned()
                    .collect(),
                ..InputRules::default()
            };
            diagnostics.extend(
                collect_exclusion_errors(&PipelineIndex::new(&checked.pipeline), &rows)
                    .into_iter()
                    .map(|(_, problem)| error(problem.to_string())),
            );
            if let Some(warning) = missing_root(spec.root.as_ref(), text) {
                diagnostics.push(warning);
            }
            finish(diagnostics, text, None)
        }
        Err(diagnostics) => {
            let external: Arc<str> = Arc::from(pipeline_text.as_str());
            let pipeline_file = crate::imports::relative_path(
                path.parent().unwrap_or_else(|| Path::new("")),
                pipeline_path,
            );
            let folder = Path::new(&pipeline_file)
                .parent()
                .unwrap_or_else(|| Path::new(""));
            let pipeline_errors = diagnostics
                .into_iter()
                .filter(Diagnostic::is_error)
                .map(|mut diagnostic| {
                    // A library's path is from the pipeline's folder.
                    match &mut diagnostic.file {
                        // An error in a library keeps its own file and text.
                        Some(file) => *file = folder.join(&*file).display().to_string(),
                        None => {
                            diagnostic.file = Some(shown.clone());
                            diagnostic.external_text = Some(Arc::clone(&external));
                        }
                    }
                    for related in &mut diagnostic.related {
                        if let Some(file) = &mut related.file {
                            *file = folder.join(&*file).display().to_string();
                        }
                    }
                    diagnostic
                })
                .collect();
            finish(pipeline_errors, text, None)
        }
    }
}

/// A warning on the recipe's `root` line when the folder it names is not
/// there, which `spit check` can say without reading any data. Not an
/// error: the recipe may be checked on one machine and run on another,
/// where the folder is; `spit inputs` stops on it where it runs.
pub(super) fn missing_root(root: Option<&(PathBuf, usize)>, text: &str) -> Option<Diagnostic> {
    let (root, line) = root?;
    if root.is_dir() {
        return None;
    }
    let written = text.lines().nth(line - 1)?;
    let folder = crate::parser::strip_comment(written)
        .trim()
        .strip_prefix("root ")?
        .trim();
    let start = written.find(folder)?;
    let message = format!(
        "dataset root `{folder}` is not a folder here; `spit inputs` and `spit dag` stop with an error unless it is one where they run"
    );
    Some(Diagnostic::new(
        Severity::Warning,
        DiagnosticSource::Pipeline,
        Some(Place::new(*line, start..start + folder.len())),
        message,
    ))
}

/// Diagnose the text of a `.spitin` recipe against `pipeline`, reading no
/// data: every rule's error at the rule, then its source paths and records.
pub fn diagnose_recipe_against(text: &str, pipeline: &Pipeline) -> Vec<Diagnostic> {
    let text = without_bom(text);
    let (spec, lines) = match crate::inputs::parse_recipe_lines(text) {
        Ok(parsed) => parsed,
        Err(parse) => {
            let diagnostic = Diagnostic::located(DiagnosticSource::Pipeline, &parse, text);
            return finish(vec![diagnostic], text, None);
        }
    };
    let mut diagnostics: Vec<_> = collect_rule_errors(pipeline, &spec.rules, &BTreeSet::new())
        .into_iter()
        .map(|(subject, error)| {
            // A rule's own errors concern its product, or its groups.
            let place = match &subject {
                DefinitionSubject::Constraint(index) => lines.rules.get(*index).map(Rule::product),
                DefinitionSubject::ConstraintGroup(index) => {
                    lines.rules.get(*index).map(Rule::dimensions)
                }
                DefinitionSubject::Exclusion(index) => lines.exclusions.get(*index).cloned(),
                _ => None,
            };
            Diagnostic::error(DiagnosticSource::Pipeline, place, error.to_string())
        })
        .collect();
    if diagnostics.is_empty() {
        let checked = match &spec.inventory {
            Some(records) => spec
                .resolve(pipeline, crate::InputSource::Inventory(records.clone()))
                .map(|_| ()),
            // A recipe without records is scanned, which finds every
            // source by its rule.
            None => spec
                .check(pipeline)
                .and_then(|()| match spec.source_without_path(pipeline) {
                    Some(product) => Err(InputError::NoSourcePath { product }),
                    None => Ok(()),
                }),
        };
        if let Err(problem) = checked {
            // Records written in the recipe keep its line numbers, and a
            // path the recipe should not give is at its `path` line.
            let place = match &problem {
                InputError::RootInBoth => spec.root.as_ref().map(|(_, line)| {
                    let written = text.lines().nth(line - 1).unwrap_or_default();
                    Place::new(*line, content_columns(written))
                }),
                InputError::Resolve(error) => {
                    error_location(pipeline, &lines, error, text, false).1
                }
                InputError::NotASource { product }
                | InputError::OutputPath { product }
                | InputError::PathInBoth { product }
                | InputError::MemberPath { product, .. } => lines.paths.get(product).cloned(),
                InputError::WithOperation { name } | InputError::WithBody { name } => {
                    lines.with_operations.get(name).cloned()
                }
                InputError::WithProduct { name } => lines.with_products.get(name).cloned(),
                _ => None,
            };
            diagnostics.push(Diagnostic::error(
                DiagnosticSource::Pipeline,
                place,
                problem.to_string(),
            ));
        }
    }
    if diagnostics.is_empty() {
        diagnostics = recipe_path_errors(pipeline, &spec, &lines, text);
    }
    finish(diagnostics, text, None)
}

/// Errors in the recipe's source path rules, its default's included, each
/// at the recipe line that writes the rule. An error placed at one of the
/// pipeline's rules, such as a collision with an output, has no place.
fn recipe_path_errors(
    pipeline: &Pipeline,
    spec: &InputSpec,
    lines: &SourceMap,
    text: &str,
) -> Vec<Diagnostic> {
    let source_paths = spec.rules.source_paths_for(pipeline);
    if source_paths.is_empty() {
        return Vec::new();
    }
    let merged = with_source_paths(pipeline, &source_paths);
    let mut places = SourceMap::default();
    for name in source_paths.keys() {
        if let Some(place) = lines.paths.get(name).or(lines.default_path.as_ref()) {
            places.paths.insert(name.clone(), place.clone());
        }
    }
    collect_paths(&merged, &places, &BTreeSet::new())
        .1
        .iter()
        .map(|error| Diagnostic::located(DiagnosticSource::Pipeline, error, text))
        .collect()
}
