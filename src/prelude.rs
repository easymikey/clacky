//! What a program asking questions imports: `use clacky::prelude::*;`.

#[cfg(feature = "derive")] pub use clacky_derive::{Choices, Form};

pub use crate::{
    core::{
        ask::{Ask, CancelAsNone, Key, Peek, Prefill, Prompt},
        askable::{Answer, Choices},
        error::{Error, Result},
        keys::{Action, KeyMap},
        session::Session,
        settings::{Defaults, OutputMode},
    },
    prompts::{
        autocomplete::{Source, autocomplete, autocomplete_multiselect},
        confirm::{Reply, confirm},
        date::date,
        group_multiselect::{GroupPick, group_multiselect},
        multiline::{SubmitKey, multiline},
        multiselect::multiselect,
        number::number,
        password::password,
        path::path,
        select::select,
        select_key::{Case, select_key},
        text::text,
    },
    render::{
        messages::Messages,
        style::{Color, Style, Theme},
        symbols::Symbols,
    },
    terminal::abort::{Abort, Stop},
    values::path::PathKind,
};
