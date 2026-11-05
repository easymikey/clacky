# Changelog

## Unreleased

## 0.2.0 (2026-11-05)

Breaking: the `tokio` feature is removed, so drop it from your `Cargo.toml` features; `ask_until`, `wait`, `async_tasks` and `Spinner::during` are always available and run on any executor. `Error::NoInput` is now `Error::NoInput { question }`, so match it as `NoInput { .. }` or read the question label from it.

### Added
- The async api works without tokio, on any runtime.
- `Error::NoInput` carries the label of the question left unanswered, and its message names that question.
- `Session::print` writes raw text between prompts.
- `password(..).secret()` returns a `SecretString` (feature `secrecy`).
- `ask_until` gives back the mapped answer.
- An intro can be a bare title.
### Changed
- One `Fillable` impl serves `Option<String>` and every `Option<number>`.

### Fixed
- A skip row stays apart from a variant labelled skip.
- An optional number is asked with the number prompt.
- A cancelled date row keeps the session's placeholders.
- The autocomplete search label is measured in columns.
- An unpaired backtick prints as it is.
- Terminal hyperlinks are measured and clipped by their text.
- The no input text is shorter and says the input gave no answer, not that stdin ended.
- A typed line that names no option asks again.
- A prompt with one row keeps the option under the cursor on screen.
- Confirm refuses a line that is neither yes nor no, with its own text.
- A Windows console that reads no escapes gets lines drawn only when needed.
- Ctrl-C puts the terminal back when it ends the program while the terminal is quiet.
- `secret` is available after `validate`, `given` and other wrappers.
- A password is copied to an exact-size box, so no unwiped block is freed.
- The futures of `wait` and `ask_until` are boxed.
- `ask_until` tries an answer nobody typed once.
- A dropped running spinner ends as canceled.
- A spinner counts only Ctrl-C presses made after it started.
- The async `tasks` run future is `Send`.

## 0.1.0 (2026-11-04)

Breaking: output text takes `impl Display`, cancel carries no causes, confirm defaults to yes, and `Confirm::or_cancel` ends a form on No.

### Added
- `Choices` and `Form` derives.
- Output: log, note, box, spinner, progress, tasks, task log and stream.
- Prompts: text, password, confirm, number, date, path, select, multiselect and autocomplete.
- Theme, symbols, messages and drawing.
- Terminal raw mode, signals, abort and line terminals.
- Session, prompts as values, keys and errors.

### Fixed
- Spinners count down without the deprecated `fetch_update`.
