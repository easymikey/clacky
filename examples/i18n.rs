//! Looks and words as values: a theme, ASCII symbols and German messages.

use clacky::prelude::*;

fn main() -> Result<()> {
    let theme = Theme::clack()
        .rail(Style::new().fg(Color::Magenta))
        .active(Style::new().fg(Color::Rgb(255, 140, 0)).bold());
    let messages = Messages::english()
        .yes("Ja")
        .no("Nein")
        .cancelled("Abgebrochen")
        .items_selected(|count| format!("{count} gewählt"));
    let ui = Session::detect()?
        .theme(theme)
        .symbols(Symbols::ascii())
        .messages(messages);

    ui.intro(" i18n ")?;
    let flavor = ui.ask(
        select("Sorte")
            .item("vanilla", "Vanille")
            .item("mint", "Minze")
            .hint("frisch"),
    )?;
    let more = ui.ask(confirm("Noch eine Kugel?"))?;
    ui.outro(format_args!("{flavor}, mehr: {more}"))
}
