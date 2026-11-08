//! Ids of the declarations a [`Pipeline`](super::Pipeline) holds in order,
//! and a column of values kept by one of them, so that what sits beside a
//! declaration, such as its place in the source, is found by its id.

use std::marker::PhantomData;

/// Declares a `u32` newtype id for the declarations in one `Pipeline` list.
macro_rules! declaration_id {
    ($(#[$doc:meta])* $name:ident, $list:literal, $limit:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(u32);

        impl $name {
            #[doc = concat!("The declaration at `index` in `", $list, "`.")]
            pub(crate) fn at(index: usize) -> Self {
                Self(u32::try_from(index).expect($limit))
            }

            #[doc = concat!("The declaration's position in `", $list, "`.")]
            pub(crate) fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl From<$name> for usize {
            fn from(id: $name) -> usize {
                id.index()
            }
        }
    };
}

declaration_id!(
    /// A `command` or `verify` line's place in `Pipeline::commands`.
    CommandId,
    "Pipeline::commands",
    "fewer than 2^32 commands in a pipeline"
);
declaration_id!(
    /// A `resources` line's place in `Pipeline::resources`.
    ResourcesId,
    "Pipeline::resources",
    "fewer than 2^32 resources lines in a pipeline"
);
declaration_id!(
    /// A `check` declaration's place in `Pipeline::checks`.
    CheckId,
    "Pipeline::checks",
    "fewer than 2^32 checks in a pipeline"
);
declaration_id!(
    /// A `review` declaration's place in `Pipeline::reviews`.
    ReviewId,
    "Pipeline::reviews",
    "fewer than 2^32 reviews in a pipeline"
);

/// One value for each declaration of a list, found by the declaration's id
/// `I`. A column may be shorter than its list, as when a pipeline was built
/// without text, so a lookup may find nothing.
#[derive(Clone, Debug)]
pub(crate) struct IdVec<I, T> {
    values: Vec<T>,
    id: PhantomData<fn(I)>,
}

impl<I, T> Default for IdVec<I, T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            id: PhantomData,
        }
    }
}

impl<I: Copy, T> IdVec<I, T>
where
    usize: From<I>,
{
    /// Add the value of declaration `id`, which is the next one: a column is
    /// written in its list's order.
    pub(crate) fn push(&mut self, id: I, value: T) {
        debug_assert_eq!(
            usize::from(id),
            self.values.len(),
            "a column is written in its list's order"
        );
        self.values.push(value);
    }

    /// The value of declaration `id`, if the column holds one.
    pub(crate) fn get(&self, id: I) -> Option<&T> {
        self.values.get(usize::from(id))
    }
}
