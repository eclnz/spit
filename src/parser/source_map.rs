use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::model::{
    CheckId, CommandId, CoverageRule, IdVec, Invocation, PathOrigin, PipelineIndex, ResourcesId,
    ReviewId,
};
use crate::span::{columns_of, content_columns, find_word, Place};

use super::keyword::Keyword;

#[derive(Clone, Debug, Default)]
pub(crate) struct SourceMap {
    pub(crate) products: BTreeMap<String, Place>,
    pub(crate) operations: BTreeMap<String, Place>,
    pub(crate) operation_stages: BTreeMap<String, String>,
    pub(crate) invocations: BTreeMap<String, Step>,
    pub(crate) constraints: BTreeMap<String, Rule>,
    pub(crate) rules: Vec<Rule>,
    pub(crate) exclusions: Vec<Place>,
    pub(crate) commands: IdVec<CommandId, CommandPlaces>,
    pub(crate) resources: IdVec<ResourcesId, Place>,
    pub(crate) checks: IdVec<CheckId, Place>,
    pub(crate) reviews: IdVec<ReviewId, Place>,
    pub(crate) review_files: Vec<Place>,
    pub(crate) default_reviews: Option<Place>,
    pub(crate) stage_reviews: BTreeMap<String, Place>,
    pub(crate) paths: BTreeMap<String, Place>,
    pub(crate) default_path: Option<Place>,
    pub(crate) stages: BTreeMap<String, Place>,
    pub(crate) default_checks: Option<Place>,
    pub(crate) stage_checks: BTreeMap<String, Place>,
    pub(crate) stage_paths: BTreeMap<String, Place>,
    pub(crate) imported: BTreeSet<String>,
    pub(crate) file_texts: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct CommandPlaces {
    pub(crate) header: Place,
    pub(crate) lines: Vec<Place>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Step {
    pub(crate) line: usize,
    pub(crate) outputs: Vec<Range<usize>>,
    pub(crate) operation: Range<usize>,
    pub(crate) call: Range<usize>,
    pub(crate) inputs: Vec<Range<usize>>,
}

impl Step {
    fn place(&self, columns: &Range<usize>) -> Place {
        Place::new(self.line, columns.clone())
    }

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

#[derive(Clone, Debug, Default)]
pub(crate) struct Rule {
    pub(crate) line: usize,
    pub(crate) whole: Range<usize>,
    pub(crate) product: Range<usize>,
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
    pub(crate) fn command(&self, id: CommandId) -> Option<Place> {
        Some(self.commands.get(id)?.header.clone())
    }

    pub(crate) fn command_line(&self, id: CommandId, line: usize) -> Option<Place> {
        self.commands.get(id)?.lines.get(line).cloned()
    }

    pub(crate) fn path_rule(&self, index: &PipelineIndex<'_>, product: &str) -> Option<Place> {
        match index.path_origin(product) {
            Some((PathOrigin::Explicit, _)) => self.paths.get(product).cloned(),
            Some((PathOrigin::Stage(stage), _)) => self.stage_paths.get(stage).cloned(),
            _ => self.default_path.clone(),
        }
    }
}

pub(super) fn name_place(original: &str, number: usize, from: &str, name: &str) -> Place {
    let start = columns_of(original, from).map_or(0, |columns| columns.start);
    Place::new(
        number,
        find_word(original, start, name).unwrap_or_else(|| content_columns(original)),
    )
}

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

pub(super) fn tail_place(original: &str, number: usize, tail: &str) -> Place {
    let content = content_columns(original);
    let columns = original[content.clone()].rfind(tail).map_or_else(
        || content.clone(),
        |offset| content.start + offset..content.start + offset + tail.len(),
    );
    Place::new(number, columns)
}

pub(super) fn rule_place(original: &str, number: usize, rule: &CoverageRule) -> Rule {
    let whole = content_columns(original);
    let content = &original[whole.clone()];
    let after_keyword =
        whole.end - Keyword::split(content).map_or(content.len(), |(_, rest)| rest.len());
    let open = original[after_keyword..whole.end]
        .find('[')
        .map(|offset| after_keyword + offset);
    let from = match open {
        Some(open) => original[open..whole.end]
            .find(']')
            .map_or(whole.end, |offset| open + offset + 1),
        None => after_keyword,
    };
    let product = find_word(original, from, &rule.product).unwrap_or_else(|| whole.clone());
    let dimensions = open.map_or_else(|| whole.clone(), |open| open..from);
    Rule {
        line: number,
        whole,
        product,
        dimensions,
    }
}
