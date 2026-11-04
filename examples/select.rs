//! Choosing from lists: select, multiselect, groups, key shortcuts and a
//! disabled option.

use clacky::prelude::*;

fn main() -> Result<()> {
    let ui = Session::detect()?;
    ui.intro("select")?;

    let region = ui.ask(
        select("Region")
            .item("eu", "Europe")
            .item("us", "United States")
            .hint("closest")
            .disabled("asia", "Asia")
            .default_at(1),
    )?;
    let tools = ui.ask(
        multiselect("Tools")
            .item("fmt", "Formatter")
            .item("lint", "Linter")
            .item("doc", "Docs")
            .picked([0, 1])
            .optional(),
    )?;
    let groups = ui.ask(
        group_multiselect("Targets")
            .group("Desktop", [("mac", "macOS"), ("win", "Windows")])
            .group("Mobile", [("ios", "iOS"), ("android", "Android")]),
    )?;
    let next = ui.ask(
        select_key("Next")
            .item('r', "run", "Run it")
            .item('b', "build", "Build only")
            .item('q', "quit", "Quit"),
    )?;

    ui.outro(format_args!(
        "{region}, {} tools, {} targets, then {next}",
        tools.len(),
        groups.len()
    ))
}
