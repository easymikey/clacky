# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
