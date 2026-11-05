# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/easymikey/clacky/compare/clacky-v0.1.0...clacky-v0.2.0) - 2026-11-05

### Added

- [**breaking**] async api without tokio, on any runtime
- *(error)* [**breaking**] name the question in NoInput
- print raw text, password secret(), mapped answers in ask_until, bare intro titles

### Changed

- *(askable)* one Fillable impl for Option<String> and Option<number>

### Fixed

- *(askable)* keep the skip row apart from a variant labelled skip
- *(askable)* ask an optional number with the number prompt
- *(date)* keep the session's placeholders on a cancelled row
- *(autocomplete)* measure the search label in columns
- *(render)* print an unpaired backtick as it is
- *(render)* measure and clip terminal hyperlinks by their text
- *(error)* shorter no input text
- *(error)* say the input gave no answer, not that stdin ended
- *(prompts)* ask again when a typed line names no option
- *(prompts)* keep the option under the cursor on screen with one row
- *(confirm)* refuse a line that is neither yes nor no with its own text
- *(terminal)* draw lines only when a Windows console reads no escapes
- *(terminal)* put the terminal back when Ctrl-C ends the program while it is quiet
- *(ask)* make secret available after validate, given and other wrappers
- *(password)* copy the secret to an exact-size box so no unwiped block is freed
- *(session)* box the futures of wait and ask_until
- *(session)* try an answer nobody typed once in ask_until
- *(spinner)* end a dropped running spinner as canceled
- *(spinner)* count only Ctrl-C presses made after the spinner started
- *(tasks)* make the async run future Send

## [0.1.0](https://github.com/easymikey/clacky/releases/tag/clacky-v0.1.0) - 2026-11-04

### Added

- [**breaking**] output text takes impl Display, cancel without causes, confirm defaults to yes, or_cancel
- *(derive)* Choices and Form derives
- *(output)* log, note, box, spinner, progress, tasks, task log and stream
- *(prompts)* text, password, confirm, number, date, path, select, multiselect, autocomplete
- *(render)* theme, symbols, messages and drawing
- *(terminal)* raw mode, signals, abort and line terminals
- *(core)* session, prompts as values, keys and errors

### Fixed

- count spinners down without the deprecated fetch_update
