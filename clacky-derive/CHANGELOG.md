# Changelog

## Unreleased

## 0.1.1 (2026-11-05)

### Added
- Print raw text, read a password with `secret()`, get mapped answers from `ask_until`, and use bare intro titles.

## 0.1.0 (2026-11-04)

Breaking: output text takes `impl Display`, cancel carries no causes, confirm defaults to yes, and `Confirm::or_cancel` ends a form on No.

### Added
- `Choices` and `Form` derives.
