use std::marker::PhantomData;

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
    CommandId,
    "Pipeline::commands",
    "fewer than 2^32 commands in a pipeline"
);
declaration_id!(
    ResourcesId,
    "Pipeline::resources",
    "fewer than 2^32 resources lines in a pipeline"
);
declaration_id!(
    CheckId,
    "Pipeline::checks",
    "fewer than 2^32 checks in a pipeline"
);
declaration_id!(
    ReviewId,
    "Pipeline::reviews",
    "fewer than 2^32 reviews in a pipeline"
);

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
    pub(crate) fn push(&mut self, id: I, value: T) {
        debug_assert_eq!(
            usize::from(id),
            self.values.len(),
            "a column is written in its list's order"
        );
        self.values.push(value);
    }

    pub(crate) fn get(&self, id: I) -> Option<&T> {
        self.values.get(usize::from(id))
    }
}
