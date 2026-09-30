//! Where declarations and their parts sit in the source, kept beside the
//! parsed pipeline so diagnostics can point at them.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::model::{CoverageRule, Invocation, Pipeline};
use crate::span::{columns_of, content_columns, find_word, Place};

use super::keyword::Keyword;

/// Where declarations sit in the source, kept beside the parsed [`Pipeline`]
/// so that diagnostics can point at them without the model carrying them.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourceMap {
    /// Each product's declared name; a step's output declares its product.
    pub(crate) products: BTreeMap<String, Place>,
    /// Each operation's declared name.
    pub(crate) operations: BTreeMap<String, Place>,
    /// Each step, keyed by the product it produces.
    pub(crate) invocations: BTreeMap<String, Step>,
    /// The last coverage rule for each product.
    pub(crate) constraints: BTreeMap<String, Rule>,
    /// One entry per `Pipeline::constraints` element, in the same order.
    pub(crate) rules: Vec<Rule>,
    /// Where each `exclude` rule written in the recipe names what it
    /// removes, in the order of `InputRules::exclusions`.
    pub(crate) exclusions: Vec<Place>,
    /// One template per `Pipeline::commands` element, in the same order.
    pub(crate) commands: Vec<Place>,
    /// Templates of `path product:` rules, keyed by product.
    pub(crate) paths: BTreeMap<String, Place>,
    /// Template of the default `path:` rule.
    pub(crate) default_path: Option<Place>,
    /// Each stage's name in its `stage` header.
    pub(crate) stages: BTreeMap<String, Place>,
    /// Templates of the `path:` rules inside stages, keyed by stage.
    pub(crate) stage_paths: BTreeMap<String, Place>,
    /// Products and operations brought in by `use` lines.
    pub(crate) imported: BTreeSet<String>,
}

/// Where the parts of one step sit on its line.
#[derive(Clone, Debug, Default)]
pub(crate) struct Step {
    pub(crate) line: usize,
    /// One range per output product, in call order.
    pub(crate) outputs: Vec<Range<usize>>,
    pub(crate) operation: Range<usize>,
    /// From the operation name to the closing parenthesis.
    pub(crate) call: Range<usize>,
    /// One range per input binding, with its selectors, in call order.
    pub(crate) inputs: Vec<Range<usize>>,
}

impl Step {
    fn place(&self, columns: &Range<usize>) -> Place {
        Place::new(self.line, columns.clone())
    }

    /// The first output product.
    pub(crate) fn output(&self) -> Place {
        self.output_at(0)
    }

    pub(crate) fn output_at(&self, index: usize) -> Place {
        let columns = self.outputs.get(index).or_else(|| self.outputs.first());
        columns.map_or_else(|| self.call(), |columns| self.place(columns))
    }

    pub(crate) fn operation(&self) -> Place {
        self.place(&self.operation)
    }

    pub(crate) fn call(&self) -> Place {
        self.place(&self.call)
    }

    pub(crate) fn input(&self, index: usize) -> Option<Place> {
        self.inputs.get(index).map(|columns| self.place(columns))
    }
}

/// Where the parts of one coverage rule sit on its line.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rule {
    pub(crate) line: usize,
    pub(crate) whole: Range<usize>,
    pub(crate) product: Range<usize>,
    /// The bracketed dimensions the rule groups by.
    pub(crate) dimensions: Range<usize>,
}

impl Rule {
    pub(crate) fn whole(&self) -> Place {
        Place::new(self.line, self.whole.clone())
    }

    pub(crate) fn product(&self) -> Place {
        Place::new(self.line, self.product.clone())
    }

    pub(crate) fn dimensions(&self) -> Place {
        Place::new(self.line, self.dimensions.clone())
    }
}

impl SourceMap {
    pub(crate) fn command(&self, index: usize) -> Option<Place> {
        self.commands.get(index).cloned()
    }

    /// The template of the path rule that `product` uses.
    pub(crate) fn path_rule(&self, pipeline: &Pipeline, product: &str) -> Option<Place> {
        if pipeline.product_paths.contains_key(product) {
            self.paths.get(product).cloned()
        } else if let Some((stage, _)) = pipeline.stage_path_rule(product) {
            self.stage_paths.get(stage).cloned()
        } else {
            self.default_path.clone()
        }
    }
}

/// Where a parsed declaration's name sits: its first whole-word occurrence
/// at or after `from`, a slice of `original`.
pub(super) fn name_place(original: &str, number: usize, from: &str, name: &str) -> Place {
    let start = columns_of(original, from).map_or(0, |columns| columns.start);
    Place::new(
        number,
        find_word(original, start, name).unwrap_or_else(|| content_columns(original)),
    )
}

/// Where a parsed step's outputs, operation, call, and inputs sit on its line.
pub(super) fn step_place(original: &str, number: usize, invocation: &Invocation) -> Step {
    let content = content_columns(original);
    let found = |from: usize, word: &str| find_word(original, from, word);
    let mut from = content.start;
    let outputs: Vec<_> = invocation
        .outputs
        .iter()
        .map(|output| {
            let columns = found(from, output).unwrap_or_else(|| content.clone());
            from = columns.end;
            columns
        })
        .collect();
    let equals = original[from..]
        .find('=')
        .map_or(from, |offset| from + offset + 1);
    let operation = found(equals, &invocation.operation).unwrap_or_else(|| content.clone());
    let call_end = original[..content.end]
        .rfind(')')
        .map_or(content.end, |index| index + 1);
    let mut from = operation.end;
    let inputs = invocation
        .inputs
        .iter()
        .map(|binding| {
            let Some(product) = found(from, binding.product_name()) else {
                return operation.start..call_end;
            };
            // Selectors run to the end of the argument.
            let end = if binding.has_selectors() {
                argument_end(original, product.end, call_end)
            } else {
                product.end
            };
            from = end;
            product.start..end
        })
        .collect();
    Step {
        line: number,
        outputs,
        call: operation.start..call_end,
        operation,
        inputs,
    }
}

/// The end of the call argument that continues at `start`: the next
/// top-level `,` or the call's closing parenthesis, less trailing space.
fn argument_end(line: &str, start: usize, call_end: usize) -> usize {
    let mut depth = 0usize;
    let mut end = call_end.saturating_sub(1).max(start);
    for (offset, character) in line[start..call_end].char_indices() {
        match character {
            '(' => depth += 1,
            ')' | ',' if depth == 0 => {
                end = start + offset;
                break;
            }
            ')' => depth -= 1,
            _ => {}
        }
    }
    start + line[start..end].trim_end().len()
}

/// Where the text at the end of a declaration sits, such as a template.
pub(super) fn tail_place(original: &str, number: usize, tail: &str) -> Place {
    let content = content_columns(original);
    let columns = original[content.clone()].rfind(tail).map_or_else(
        || content.clone(),
        |offset| content.start + offset..content.start + offset + tail.len(),
    );
    Place::new(number, columns)
}

/// Where a parsed coverage rule's product and grouped dimensions sit.
pub(super) fn rule_place(original: &str, number: usize, rule: &CoverageRule) -> Rule {
    let whole = content_columns(original);
    let content = &original[whole.clone()];
    let after_keyword =
        whole.end - Keyword::split(content).map_or(content.len(), |(_, rest)| rest.len());
    let product =
        find_word(original, after_keyword, &rule.product).unwrap_or_else(|| whole.clone());
    let dimensions = original[product.end..whole.end]
        .find('[')
        .map_or_else(|| whole.clone(), |offset| product.end + offset..whole.end);
    Rule {
        line: number,
        whole,
        product,
        dimensions,
    }
}
