use rustc_hash::FxHashSet;

use crate::model::Pipeline;
use crate::parser::error::LowerError;

use super::Module;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeclKind {
    Product,
    Operation,
    Command,
    Resources,
    ProductPath,
    SidecarGroup,
    Check,
    Type,
    Review,
}

const _: () = {
    let mut index = 0;
    while index < DeclKind::ALL.len() {
        assert!(
            DeclKind::ALL[index] as usize == index,
            "DeclKind::ALL is in declaration order"
        );
        index += 1;
    }
    assert!(
        DeclKind::ALL.len() == DeclKind::Review as usize + 1,
        "DeclKind::ALL ends at the last variant"
    );
};

impl DeclKind {
    pub(super) const ALL: &'static [Self] = &[
        Self::Product,
        Self::Operation,
        Self::Command,
        Self::Resources,
        Self::ProductPath,
        Self::SidecarGroup,
        Self::Check,
        Self::Type,
        Self::Review,
    ];

    fn noun(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::Operation => "operation",
            Self::Command => "command for operation",
            Self::Resources => "resources for operation",
            Self::ProductPath => "path for product",
            Self::SidecarGroup => "source companions",
            Self::Check => "check",
            Self::Type => "type",
            Self::Review => "review",
        }
    }

    fn names(self, pipeline: &Pipeline) -> Vec<&str> {
        match self {
            Self::Product => (pipeline.products.iter())
                .map(|product| product.name.as_str())
                .collect(),
            Self::Operation => (pipeline.operations.iter())
                .map(|operation| operation.name.as_str())
                .collect(),
            Self::Command => (pipeline.commands.iter())
                .map(|command| command.operation.as_str())
                .collect(),
            Self::Resources => (pipeline.resources.iter())
                .map(|line| line.operation.as_str())
                .collect(),
            Self::ProductPath => pipeline.product_paths.keys().map(String::as_str).collect(),
            Self::SidecarGroup => (pipeline.sidecar_groups.iter())
                .map(|group| group.name.as_str())
                .collect(),
            Self::Check => (pipeline.checks.iter())
                .map(|check| check.name.as_str())
                .collect(),
            Self::Type => pipeline.type_decls.keys().map(String::as_str).collect(),
            Self::Review => (pipeline.reviews.iter())
                .map(|review| review.name.as_str())
                .collect(),
        }
    }

    fn conflict<'i>(self, pipeline: &Pipeline, imported: &'i Pipeline) -> Option<&'i str> {
        match self {
            Self::Product
            | Self::Operation
            | Self::Command
            | Self::Resources
            | Self::ProductPath
            | Self::SidecarGroup => {
                let existing: FxHashSet<_> = self.names(pipeline).into_iter().collect();
                self.names(imported)
                    .into_iter()
                    .find(|name| existing.contains(name))
            }
            Self::Check => imported.checks.iter().enumerate().find_map(|(at, check)| {
                (pipeline.checks.iter())
                    .chain(&imported.checks[..at])
                    .find(|earlier| earlier.name == check.name)
                    .is_some_and(|earlier| earlier != check)
                    .then_some(check.name.as_str())
            }),
            Self::Type => imported.type_decls.iter().find_map(|(name, params)| {
                (pipeline.type_decls.get(name))
                    .is_some_and(|earlier| earlier != params)
                    .then_some(name.as_str())
            }),
            Self::Review => None,
        }
    }

    pub(super) fn selectable(self, module: &Module<'_>, name: &str) -> bool {
        match self {
            Self::Product => module.sources.contains_key(name),
            Self::Operation => module.operations.contains_key(name),
            Self::SidecarGroup => module.groups.contains_key(name),
            Self::Check => module.checks.contains_key(name),
            Self::Review => module.reviews.contains(name),
            Self::Command | Self::Resources | Self::ProductPath | Self::Type => false,
        }
    }
}

pub(super) fn first_conflict(pipeline: &Pipeline, imported: &Pipeline) -> Option<LowerError> {
    DeclKind::ALL.iter().find_map(|&kind| {
        let name = kind.conflict(pipeline, imported)?;
        Some(LowerError::ImportConflict {
            noun: kind.noun().to_owned(),
            name: name.to_string(),
        })
    })
}
