# Look and terminals

Theme, symbols, messages and key map are values on the session, so two sessions never share them.

```
use clacky::prelude::*;

let theme = Theme::clack().rail(Style::new().fg(Color::Magenta));
let messages = Messages::english().yes("Ja").no("Nein");
let keys = KeyMap::clack().bind(Key::Char('n'), Action::Down);
assert_eq!(keys.action(Key::Char('n')), Some(Action::Down));

let session = Session::detect().map(|ui| {
    ui.theme(theme)
        .symbols(Symbols::ascii())
        .messages(messages)
        .keys(keys)
        .output(OutputMode::Accessible)
});
# drop(session);
```

`OutputMode::Accessible` prints changed rows as new lines instead of redrawing, and `Session::detect` picks it for `TERM=dumb`. There choices are a numbered list. With piped stdin, `text`, `password`, `date` and `path` read the next line and the other prompts are `Error::NoInput`, as is a stdin that ends, never the default. The cursor and terminal settings are restored on every exit.
