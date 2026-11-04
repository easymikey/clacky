//! A path and a date, both checked as they are typed.

use clacky::{Date, DateOrder, prelude::*};

fn main() -> Result<()> {
    let ui = Session::detect()?;
    ui.intro(" files and dates ")?;

    let Some(earliest) = Date::new(2026, 1, 1) else {
        return ui.cancel("No such date", &[]);
    };
    let config = ui.ask(
        path("Config file")
            .kind(PathKind::Existing)
            .default("config.toml"),
    )?;
    let folder = ui.ask(path("Output folder").kind(PathKind::Directory))?;
    let day = ui.ask(date("Departure").order(DateOrder::Dmy).min(earliest))?;

    ui.outro(&format!(
        "{} into {} on {day:?}",
        config.display(),
        folder.display()
    ))
}
