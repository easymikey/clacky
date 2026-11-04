//! Search while typing: autocomplete, its multiple form and a custom match
//! rule.

use clacky::{Choice, prelude::*};

fn main() -> Result<()> {
    let ui = Session::detect()?;
    ui.intro("search")?;

    let one = ui.ask(
        autocomplete("Crate")
            .item("serde", "serde")
            .item("tokio", "tokio")
            .item("rand", "rand")
            .disabled("openssl", "openssl")
            .hint("type to narrow"),
    )?;
    let many = ui.ask(
        autocomplete_multiselect("Crates")
            .item("serde", "serde")
            .item("tokio", "tokio")
            .item("rand", "rand")
            .picked([0])
            .optional(),
    )?;

    let names = ["build", "bench", "check", "clippy"];
    let choices: Vec<_> = names.iter().map(|name| Choice::new(*name, *name)).collect();
    let prefixes = Source::from(choices).matching(|typed| {
        names
            .iter()
            .enumerate()
            .filter(|(_, name)| name.starts_with(typed))
            .map(|(place, _)| place)
            .collect()
    });
    let command = ui.ask(autocomplete("Command").source(prefixes))?;

    ui.outro(format_args!("{one}, {} more, then {command}", many.len()))
}
