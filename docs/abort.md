# Cancel and abort

Esc is `Error::Cancelled` and Ctrl-C is `Error::Interrupted`;
`Error::is_cancel` is true for both. `.cancel_as_none()` turns Esc into
`None` and leaves Ctrl-C an error.

Another thread or a timer can end a question that waits for the person.
`Abort::new()` gives an `Abort` and the `Stop` that ends it; `.abort(&abort)`
ties a question to it and `.timeout(duration)` to a deadline. Whichever ends
first wins, the answer is `Error::Aborted`, and the terminal is put back.

```no_run
use std::{thread, time::Duration};

use clacky::prelude::*;

# fn main() -> Result<()> {
let ui = Session::detect()?;
let (abort, stop) = Abort::new();
thread::spawn(move || {
    thread::sleep(Duration::from_secs(30));
    stop.stop();
});
let asked = ui.ask(text("Name").abort(&abort).timeout(Duration::from_secs(10)));
if matches!(asked, Err(Error::Aborted)) {
    return ui.cancel("Too slow", &[]);
}
# Ok(())
# }
```
