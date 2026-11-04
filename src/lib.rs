//! Clack-style terminal prompts: one question at a time on a rail.
//!
//! A prompt is a value; a [`Session`](core::session::Session) runs it and owns
//! the look, so nothing is global.
//!
//! ```no_run
//! use clacky::prelude::*;
//!
//! fn main() -> Result<()> {
//!     let ui = Session::detect()?;
//!     ui.intro(" my-app ")?;
//!     let name = ui.ask(text("Project name").default("my-app"))?;
//!     let port: u16 = ui.ask(number("Port").min(1).max(65535).default(8080))?;
//!     ui.outro(&format!("{name} on {port}"))
//! }
//! ```
//!
//! `Session::detect` reads the environment (`TERM`, `NO_COLOR` and the like)
//! once. The `prelude` has what a program asking questions needs; types that
//! only some programs name, such as `Tone` or `DateOrder`, sit at the root.
//!
//! The [`guide`] has a page per topic: [prompts](guide::prompts),
//! [forms with derive](guide::forms), [live output](guide::live_output),
//! [look and terminals](guide::theming), [abort and timeout](guide::abort),
//! [testing](guide::testing) and [size and comparison](guide::size).
//! `examples/` has a program per topic.
//!
//! | Feature | Default | What it adds |
//! |---|---|---|
//! | `tokio` | yes | `Session::ask_until`, `wait`, `AsyncTasks`, `Spinner::during` |
//! | `derive` | no | `#[derive(Choices)]`, `#[derive(Form)]` |
//! | `fake` | no | `clacky::fake`, the scripted terminal and screen emulator |
#![deny(missing_docs)]

/// The README, compiled: every code block in it is a doc test.
#[cfg(all(doctest, feature = "fake", feature = "derive", feature = "tokio"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// One page per topic; every snippet on them is compiled as a doctest.
#[cfg(any(doc, doctest))]
pub mod guide {
    #[doc = include_str!("../docs/prompts.md")]
    pub mod prompts {}

    #[cfg(feature = "derive")]
    #[doc = include_str!("../docs/forms.md")]
    pub mod forms {}

    #[doc = include_str!("../docs/live-output.md")]
    pub mod live_output {}

    #[doc = include_str!("../docs/theming.md")]
    pub mod theming {}

    #[doc = include_str!("../docs/abort.md")]
    pub mod abort {}

    #[cfg(feature = "fake")]
    #[doc = include_str!("../docs/testing.md")]
    pub mod testing {}

    #[doc = include_str!("../docs/size.md")]
    pub mod size {}
}

pub mod core;
#[cfg(any(test, feature = "fake"))] pub mod fake;
pub mod output;
pub mod prelude;
pub mod prompts;
pub mod render;
pub mod terminal;
pub mod values;

pub use core::{
    error::{Error, Result},
    keys::{Action, KeyMap},
    settings::OutputMode,
};

#[cfg(feature = "derive")] pub use clacky_derive::{Choices, Form};
#[cfg(feature = "tokio")] pub use output::tasks::AsyncTasks;
pub use output::{
    LogEntry,
    boxed::{Align, BoxWidth, Boxed, Corners},
    progress::{BarStyle, Progress},
    spinner::{Indicator, Spinner},
    stream::LogStream,
    task_log::{TaskLog, TaskLogGroup},
    tasks::Tasks,
};
pub use prompts::{
    autocomplete::{
        Autocomplete,
        AutocompleteMultiSelect,
        EmptyAutocomplete,
        EmptyAutocompleteMultiSelect,
        Source,
    },
    choice::Choice,
    confirm::{Confirm, Reply},
    date::DatePrompt,
    group_multiselect::{EmptyGroupMultiSelect, GroupMultiSelect, GroupPick},
    multiline::{Multiline, SubmitKey},
    multiselect::{EmptyMultiSelect, MultiSelect},
    number::{Number, NumberNotice, NumberState, Numeric},
    password::{OnError, Password},
    path::PathPrompt,
    select::{EmptySelect, Select},
    select_key::{Case, EmptySelectKey, SelectKey},
    text::Text,
};
pub use render::{
    draw::{NoteLine, NoteTone, Tone},
    messages::{Messages, Refusal},
    style::{BaseColor, Color, ColorDepth, Style, Theme},
    symbols::Symbols,
};
pub use terminal::abort::{Abort, Stop};
pub use values::{
    date::{Date, DateOrder},
    line::CharFilter,
};
