//! Accessible output: plain lines that a screen reader can follow, with no
//! redraws.

use clacky::prelude::*;

fn main() -> Result<()> {
    let ui = Session::detect()?
        .output(OutputMode::Accessible)
        .without_rail();
    ui.intro("accessible")?;

    let kind = ui.ask(select("Kind").item("lib", "Library").item("bin", "Binary"))?;
    let ok = ui.ask(confirm("Create it?"))?;

    let mut spinner = ui.spinner();
    spinner.start("Creating")?;
    spinner.stop("Created")?;

    ui.outro(format_args!("{kind}: {ok}"))
}
