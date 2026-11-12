//! The kinds of declaration an import brings in, and how each meets a
//! declaration of the same name already in the importing file.

use rustc_hash::FxHashSet;

use crate::model::Pipeline;

use super::Module;

/// A kind of declaration an import brings in. Each `match` on it is
/// exhaustive, so a new kind must say how it conflicts and how a `use` line
/// selects it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeclKind {
    /// A product: a source or a step's output.
    Product,
    /// An `operation` declaration.
    Operation,
    /// A `command` or `verify` line, by the operation it is for.
    Command,
    /// A `resources` line, by the operation it is for.
    Resources,
    /// A `path product:` rule, by its product.
    ProductPath,
    /// A main source with its `beside` companions, by the main source.
    SidecarGroup,
    /// A `check` declaration.
    Check,
    /// A type declared by a `types` line.
    Type,
    /// A `review` declaration. It is the last variant, which the check on
    /// [`DeclKind::ALL`] relies on.
    Review,
}

// Fails the build unless `DeclKind::ALL` holds each variant once, in
// declaration order: each entry sits at its own discriminant, and the list
// ends at the last variant, so none is left out. An unnamed constant is
// evaluated whether or not anything uses it.
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
    /// Every kind, in the order an import's conflicts are looked for, which
    /// decides the one reported when an import has several. It lists the
    /// variants in declaration order, which a constant beside the enum
    /// checks when the crate builds.
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

    /// What a conflict calls a declaration of this kind, before its name.
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

    /// The names `pipeline` declares of this kind, in declaration order.
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

    /// The first name of this kind that `imported` may not bring into
    /// `pipeline`.
    fn conflict<'i>(self, pipeline: &Pipeline, imported: &'i Pipeline) -> Option<&'i str> {
        match self {
            // Any second declaration of a name is a conflict.
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
            // Two imports may bring the same check, which is one check; a
            // different check of the same name is a conflict, even within
            // one import.
            Self::Check => imported.checks.iter().enumerate().find_map(|(at, check)| {
                // The first of the name is the one the import is compared to.
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
            // A review is only a name, so two of one name are one review.
            Self::Review => None,
        }
    }

    /// Whether `module` declares a name of this kind that a `use` line may
    /// select by that name.
    pub(super) fn selectable(self, module: &Module<'_>, name: &str) -> bool {
        match self {
            Self::Product => module.sources.contains_key(name),
            Self::Operation => module.operations.contains_key(name),
            Self::SidecarGroup => module.groups.contains_key(name),
            Self::Check => module.checks.contains_key(name),
            Self::Review => module.reviews.contains(name),
            // Brought in with the operation or product they belong to, or,
            // for types, with every import of their file.
            Self::Command | Self::Resources | Self::ProductPath | Self::Type => false,
        }
    }
}

/// The message for the first declaration `imported` may not bring into
/// `pipeline`, if there is one.
pub(super) fn first_conflict(pipeline: &Pipeline, imported: &Pipeline) -> Option<String> {
    DeclKind::ALL.iter().find_map(|&kind| {
        let name = kind.conflict(pipeline, imported)?;
        Some(format!("import conflicts with {} `{name}`", kind.noun()))
    })
}
