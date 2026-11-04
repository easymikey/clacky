# Terminals without a keyboard

| Input | What happens |
|---|---|
| terminal on stdin and stderr | keys, redraws, cursor |
| `TERM=dumb` | choices are a numbered list, answered by number or label; no cursor movement |
| stdin piped | `text`, `password`, `date` and `path` read the next line; the rest is `Error::NoInput` |
| stdin ends | `Error::NoInput`, never the default |
| stderr piped, stdin a terminal | still asked, output carries no escapes |

The cursor and the terminal settings are restored on every exit: an error, a
panic or a dropped value. The terminal is quiet (no echo) only while a
question or live output runs. SIGTERM and SIGHUP during that put the
terminal back and then end the program with the signal's own exit status
(Unix).

The [testing guide](crate::guide::testing) shows a select answered by number on
a scripted `Dumb` terminal.
