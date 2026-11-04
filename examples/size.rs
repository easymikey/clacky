//! A small form to measure: `just size` prints the size of its release build.

use std::hint::black_box;

use clacky::prelude::*;

fn main() -> Result<()> {
    let ui = Session::detect()?;
    let answers = ui.ask((
        text("Project name"),
        select("Kind").item("lib", "Library").item("bin", "Binary"),
        confirm("Create it?"),
    ))?;
    black_box(answers);
    Ok(())
}
