# Look and feel

Theme, symbols, messages and key map are values on the session; two sessions
never share them. `Theme`, `Symbols` and `Messages` keep their fields public
and have one builder method per field. A text setter takes a literal or a
`String`, and a text that depends on a value takes a function.

```
use clacky::prelude::*;

let theme = Theme::clack().rail(Style::new().fg(Color::Magenta));
let messages = Messages::english()
    .yes("Ja")
    .no("Nein")
    .items_selected(|count| format!("{count} gewählt"));
assert_eq!(messages.yes, "Ja");
assert_eq!((messages.items_selected)(2), "2 gewählt");

let keys = KeyMap::clack()
    .unbind(Key::Char('j'))
    .bind(Key::Char('n'), Action::Down);
assert_eq!(keys.action(Key::Char('n')), Some(Action::Down));

let session = Session::detect().map(|ui| {
    ui.theme(theme)
        .symbols(Symbols::ascii())
        .messages(messages)
        .keys(keys)
        .without_rail()
        .output(OutputMode::Accessible)
});
# drop(session);
```

`Theme::clack()` and `Theme::mono()` are the two shipped themes, and a
builder chain makes your own. Colors fall back by terminal depth (truecolor
to 256 to 16). `OutputMode::Accessible` prints changed rows as new lines
instead of redrawing, and `Session::detect` picks it for `TERM=dumb`.
