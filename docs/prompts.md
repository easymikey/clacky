# Prompts

Every prompt is a free function that returns a builder. `ui.ask(prompt)`
runs it and gives back the answer.

| Function | Answer |
|---|---|
| `text`, `password`, `multiline` | `String` (a password is wiped on drop) |
| `confirm` | `bool` |
| `number` | the number type, `i32` when nothing else says |
| `date`, `path` | `Date`, `PathBuf` |
| `select`, `select_key` | the value of the chosen option |
| `multiselect`, `group_multiselect` | the values of the picked options |
| `autocomplete`, `autocomplete_multiselect` | one or several values, found by typing |

```no_run
use clacky::{DateOrder, prelude::*};

# fn main() -> Result<()> {
let ui = Session::detect()?;
let name = ui.ask(text("Name").placeholder("Ada"))?;
let secret = ui.ask(password("Password").mask('*'))?;
let ok = ui.ask(confirm("Sure?").labels("Yes please", "No thanks"))?;
let day = ui.ask(date("Departure").order(DateOrder::Dmy))?;
let dir = ui.ask(path("Folder").kind(PathKind::Directory))?;
let port = ui.ask(
    select("Port")
        .item(8080, "Development")
        .item(443, "Production"),
)?;
let tools = ui.ask(
    multiselect("Tools")
        .item("fmt", "Formatter")
        .item("lint", "Linter")
        .picked([0])
        .optional(),
)?;
# drop((name, secret, ok, day, dir, port, tools));
# Ok(())
# }
```


## Checking and shaping answers

Any prompt takes `.validate(check)`, where the check returns `Ok(())` or
`Err(message)`; a refused answer keeps the question open under the message.
Text-like prompts take `.parse::<T>()` and a number prompt already answers
with the number. `.map(f)` changes the answer, and a tuple of prompts (up to
eight) is asked in order and answers with a tuple.

`number` takes its type from `min`, `max`, `default`, `step` or the binding
(`let port: u16 = ..`), or from `number::<u16>(..)`. Up and Down step, and
Enter on nothing takes the default.

`simulate` runs a prompt on a list of keys without a terminal, which is
also the quickest way to try one. It is a method of the prompt itself, so it
comes before `.map` or a tuple:

```
use clacky::prelude::*;

# fn main() -> Result<()> {
let keys = "a@b".chars().map(Key::Char).chain([Key::Enter]);
let email = text("Email").validate(|address: &str| {
    if address.contains('@') {
        Ok(())
    } else {
        Err("Needs an @.")
    }
});
assert_eq!(email.simulate(keys)?, "a@b");

let kind = select("Kind").item(1, "Library").item(2, "Binary");
assert_eq!(kind.simulate([Key::Down, Key::Enter])?, 2);
# Ok(())
# }
```
