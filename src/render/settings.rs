//! One field list per settings struct: the struct and a builder method per
//! field come from the same lines, so the two cannot drift apart.

/// Declares a settings struct whose fields stay public and get a `#[must_use]`
/// setter each, named like the field, with the field's doc.
macro_rules! settings {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $(
                $(#[$doc:meta])*
                $field:ident : $ty:ty $(=> $param:ty)?,
            )*
        }
    ) => {
        $(#[$meta])*
        pub struct $name {
            $(
                $(#[$doc])*
                pub $field: $ty,
            )*
        }

        impl $name {
            $(
                $crate::render::settings::setter! {
                    [$(#[$doc])*] $field: $ty $(=> $param)?
                }
            )*
        }
    };
}

/// One setter; see [`settings`].
macro_rules! setter {
    ([$(#[$doc:meta])*] $field:ident : $ty:ty) => {
        $(#[$doc])*
        #[must_use]
        pub const fn $field(mut self, value: $ty) -> Self {
            self.$field = value;
            self
        }
    };
    ([$(#[$doc:meta])*] $field:ident : $ty:ty => $param:ty) => {
        $(#[$doc])*
        #[must_use]
        pub fn $field(mut self, value: impl Into<$param>) -> Self {
            self.$field = value.into();
            self
        }
    };
}

pub(crate) use setter;
pub(crate) use settings;
