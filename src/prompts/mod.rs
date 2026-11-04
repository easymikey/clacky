//! One module per prompt; each holds the builder and its keys-to-state machine.

pub mod autocomplete;
mod caret;
pub mod choice;
#[cfg(doctest)] mod compile_fail;
pub mod confirm;
pub mod date;
pub mod group_multiselect;
pub mod multiline;
pub mod multiselect;
pub mod number;
pub mod password;
pub mod path;
pub mod select;
pub mod select_key;
pub mod text;
