//! The `path` prompt: a file or directory path typed with Tab completion.

use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt, Redirected},
        error::Result,
    },
    prompts::{
        caret,
        choice::{radio_row, rail},
    },
    render::messages::{Messages, Refusal},
    values::{
        line::{Cancellation, CharFilter, TypedAnswer},
        path::{
            EntryKind,
            Files,
            Listings,
            PathKind,
            PathNotice,
            StdFiles,
            complete,
            is_separator,
            suggest,
        },
    },
};

const ROWS: usize = 5;

/// A path, completed with Tab from the directory it is typed in; Up and Down
/// pick a suggestion, and directories end with the separator.
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let dir = ui.ask(path("Project folder?").kind(PathKind::Directory))?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn path(message: impl Into<String>) -> PathPrompt<'static> {
    PathPrompt {
        message: message.into(),
        files: &StdFiles,
        root: None,
        kind: PathKind::Any,
        home: None,
        default: None,
        listings: Listings::default(),
    }
}

/// The question [`path`] builds.
#[derive(Debug)]
pub struct PathPrompt<'f> {
    message: String,
    files: &'f dyn Files,
    root: Option<PathBuf>,
    kind: PathKind,
    home: Option<PathBuf>,
    default: Option<PathBuf>,
    listings: Listings,
}

impl PathPrompt<'_> {
    /// Where directories are listed and paths checked; the real file system by
    /// default.
    pub fn files<F: Files>(self, files: &F) -> PathPrompt<'_> {
        PathPrompt {
            message: self.message,
            files,
            root: self.root,
            kind: self.kind,
            home: self.home,
            default: self.default,
            listings: Listings::default(),
        }
    }

    /// The directory a relative path is relative to; the answer is joined to
    /// it. By default the current directory, and the answer stays as typed.
    #[must_use]
    pub fn root(self, root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
            ..self
        }
    }

    /// Which paths are accepted; any by default.
    #[must_use]
    pub fn kind(self, kind: PathKind) -> Self {
        Self { kind, ..self }
    }

    /// What a leading `~` stands for. Without it `~` is an ordinary character.
    #[must_use]
    pub fn home(self, home: Option<PathBuf>) -> Self {
        Self { home, ..self }
    }

    /// The answer when Enter is pressed on nothing.
    #[must_use]
    pub fn default(self, path: impl Into<PathBuf>) -> Self {
        Self {
            default: Some(path.into()),
            ..self
        }
    }

    fn expand(&self, typed: &str) -> String {
        let tilde = self
            .home
            .as_ref()
            .zip(typed.strip_prefix('~'))
            .filter(|(_, rest)| rest.is_empty() || rest.starts_with(is_separator));
        if let Some((home, rest)) = tilde {
            return home
                .join(rest.trim_start_matches(is_separator))
                .to_string_lossy()
                .into_owned();
        }
        typed.to_owned()
    }

    fn state(&self, line: TypedAnswer, picked: usize) -> Located {
        let expanded = PathBuf::from(self.expand(line.text()));
        let path = match &self.root {
            Some(root) => root.join(expanded),
            None => expanded,
        };
        Located { line, picked, path }
    }

    fn suggestions(&self, typed: &str) -> Vec<String> {
        if typed == "~" {
            return Vec::new();
        }
        let root = self.root.as_deref().unwrap_or_else(|| Path::new("."));
        let files = self.listings.of(self.files);
        suggest(&self.expand(typed), root, self.kind, &files)
    }

    fn completed(&self, state: &Located) -> Option<String> {
        let typed = state.line.text();
        if typed == "~" && self.home.is_some() {
            return Some(format!("~{MAIN_SEPARATOR}"));
        }
        self.suggestions(typed)
            .get(state.picked)
            .map(|name| complete(typed, name))
    }

    fn submit(&self, state: Located) -> Next<Located> {
        let line = state.line.trimmed();
        let line = match &self.default {
            Some(default) if line.text().is_empty() => {
                TypedAnswer::new(CharFilter::Printable)
                    .prefilled(&default.to_string_lossy())
            }
            Some(_) | None => line,
        };
        let next = self.state(line, 0);
        let shown = next.line.text();
        let found = self.files.kind(&Path::new(".").join(&next.path));
        let refusal = if shown.is_empty() {
            Some(PathNotice::Empty)
        } else {
            match (self.kind, found) {
                (PathKind::Any, _)
                | (PathKind::Existing, Some(_))
                | (PathKind::Directory, Some(EntryKind::Directory)) => None,
                (PathKind::Existing | PathKind::Directory, None) => {
                    Some(PathNotice::Missing(shown.to_owned()))
                }
                (PathKind::Directory, Some(EntryKind::File | EntryKind::Other)) => {
                    Some(PathNotice::NotDirectory(shown.to_owned()))
                }
            }
        };
        match refusal {
            Some(why) => Next::Refuse(next, Refusal::Path(why)),
            None => Next::Submit(next),
        }
    }

    fn suggestion_rows(&self, state: &Located, bar: &str, frame: &mut Frame) {
        let found = self.suggestions(state.line.text());
        let first = (state.picked + 1).saturating_sub(ROWS);
        let rows: Vec<String> = found
            .iter()
            .enumerate()
            .skip(first)
            .take(ROWS)
            .map(|(at, name)| {
                let row = radio_row(frame.skin(), at == state.picked, name, None);
                format!("{bar}  {row}")
            })
            .collect();
        frame.extend(rows);
    }
}

/// What the person has typed and which suggestion is picked.
#[derive(Debug)]
pub struct Located {
    line: TypedAnswer,
    picked: usize,
    path: PathBuf,
}

impl Prompt for PathPrompt<'_> {
    type Output = PathBuf;
    type State = Located;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<Located> {
        Ok(self.state(TypedAnswer::new(CharFilter::Printable), 0))
    }

    fn update(&self, state: Located, key: Key) -> Next<Located> {
        match key {
            Key::Enter => self.submit(state),
            Key::Interrupt => Next::Quit(state, Cancellation::Interrupt),
            Key::Cancel => Next::Quit(state, Cancellation::Cancel),
            Key::EndOfInput if state.line.text().is_empty() => {
                Next::Quit(state, Cancellation::Cancel)
            }
            Key::Tab => self.completed(&state).map_or_else(
                || Next::Editing(state),
                |text| {
                    let line = TypedAnswer::new(CharFilter::Printable).prefilled(&text);
                    Next::Editing(self.state(line, 0))
                },
            ),
            Key::Up | Key::Down => {
                let count = self.suggestions(state.line.text()).len();
                let picked = if key == Key::Up {
                    state
                        .picked
                        .checked_sub(1)
                        .unwrap_or(count.saturating_sub(1))
                } else if state.picked.saturating_add(1) < count {
                    state.picked.saturating_add(1)
                } else {
                    0
                };
                Next::Editing(Located { picked, ..state })
            }
            Key::Char(_)
            | Key::EndOfInput
            | Key::Backspace
            | Key::Delete
            | Key::Left
            | Key::Right
            | Key::WordLeft
            | Key::WordRight
            | Key::Home
            | Key::End
            | Key::ClearBefore
            | Key::ClearAfter
            | Key::DeleteWord
            | Key::Ignored => Next::Editing(self.state(state.line.edit(key), 0)),
        }
    }

    fn view(&self, state: &Located, phase: Phase<'_>, frame: &mut Frame) {
        match phase {
            Phase::Active | Phase::Warned(_) => {
                let bar = rail(frame.skin(), phase);
                let room = frame.width().map(|width| width.saturating_sub(4).max(1));
                let rows = caret::rows(
                    frame.skin(),
                    (state.line.text(), state.line.cursor()),
                    "",
                    room,
                );
                frame.extend(rows.into_iter().map(|row| format!("{bar}  {row}")));
                self.suggestion_rows(state, &bar, frame);
            }
            Phase::Submitted | Phase::Cancelled => frame.row(state.line.text()),
        }
    }

    fn finish(self, state: Located) -> Result<PathBuf> {
        Ok(state.path)
    }

    fn redirected(&self) -> Redirected {
        Redirected::Line
    }

    fn by_default(&self) -> Option<Located> {
        self.default
            .is_some()
            .then(|| self.state(TypedAnswer::new(CharFilter::Printable), 0))
    }
}

impl Peek for PathPrompt<'_> {
    type Peeked<'a>
        = &'a Path
    where
        Self: 'a;

    fn peek<'a>(&'a self, state: &'a Located) -> Option<&'a Path> {
        Some(&state.path)
    }
}

impl Prefill for PathPrompt<'_> {
    fn shown_for(&self, answer: &PathBuf, _messages: &Messages) -> Option<String> {
        Some(answer.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        io,
        path::{MAIN_SEPARATOR, Path, PathBuf},
    };

    use crate::{
        core::{
            ask::{Key, Next, Peek, Prompt},
            error::Error,
        },
        prompts::path::path,
        render::messages::Refusal,
        values::{
            line::{CharFilter, TypedAnswer},
            path::{DirEntry, FakeFiles, Files, PathKind, PathNotice},
        },
    };

    #[derive(Debug)]
    struct Counting {
        inner: FakeFiles,
        lists: Cell<usize>,
    }

    impl Files for Counting {
        fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>> {
            self.lists.set(self.lists.get() + 1);
            self.inner.list(dir)
        }
    }

    const SEP: char = MAIN_SEPARATOR;

    fn fake() -> FakeFiles {
        FakeFiles::new(&[
            "/r/src/main.rs",
            "/r/src/lib.rs",
            "/r/docs/",
            "/r/Cargo.toml",
        ])
    }

    fn chars(text: &str) -> Vec<Key> {
        text.chars().map(Key::Char).collect()
    }

    fn typed(text: &str, more: impl IntoIterator<Item = Key>) -> Vec<Key> {
        chars(text)
            .into_iter()
            .chain(more)
            .chain([Key::Enter])
            .collect()
    }

    #[test]
    fn tab_completes_the_highlighted_suggestion_and_goes_inside_a_directory() {
        let files = fake();
        let question = path("Where?").files(&files).root("/r");
        let keys = typed("s", [Key::Tab, Key::Char('m'), Key::Tab]);
        assert_eq!(
            question.simulate(keys).ok(),
            Some(PathBuf::from("/r").join(format!("src{SEP}main.rs")))
        );
    }

    #[test]
    fn down_picks_the_next_suggestion() {
        let files = fake();
        let question = path("Where?").files(&files).root("/r");
        // Directories first: docs/, src/, then Cargo.toml.
        let keys = typed("", [Key::Down, Key::Tab]);
        assert_eq!(
            question.simulate(keys).ok(),
            Some(PathBuf::from("/r").join(format!("src{SEP}")))
        );
    }

    #[test]
    fn directory_kind_suggests_only_directories() {
        let files = fake();
        let question = path("Where?")
            .files(&files)
            .root("/r")
            .kind(PathKind::Directory);
        let state = question.start().ok();
        assert_eq!(
            question.suggestions(""),
            vec![format!("docs{SEP}"), format!("src{SEP}")]
        );
        assert!(state.is_some());
    }

    #[test]
    fn tilde_expands_with_a_home_and_stays_without() {
        let files = FakeFiles::new(&["/h/notes.txt"]);
        let homed = path("Where?").files(&files).home(Some(PathBuf::from("/h")));
        assert_eq!(
            homed.simulate(typed("~/n", [Key::Tab])).ok(),
            Some(PathBuf::from("/h/notes.txt"))
        );
        let homeless = path("Where?").files(&files).home(None);
        assert_eq!(homeless.suggestions("~/n"), Vec::<String>::new());
        assert_eq!(
            homeless.simulate(typed("~/n", [])).ok(),
            Some(PathBuf::from("~/n"))
        );
    }

    #[test]
    fn tab_on_a_bare_tilde_goes_to_the_home_directory() {
        let files = FakeFiles::new(&["/h/notes.txt"]);
        let homed = path("Where?").files(&files).home(Some(PathBuf::from("/h")));
        assert_eq!(
            homed.simulate(typed("~", [Key::Tab])).ok(),
            Some(PathBuf::from("/h/"))
        );
        let homeless = path("Where?").files(&files);
        assert_eq!(
            homeless.simulate(typed("~", [Key::Tab])).ok(),
            Some(PathBuf::from("~"))
        );
    }

    #[test]
    fn a_directory_is_listed_once_per_prompt_run() {
        let files = Counting {
            inner: fake(),
            lists: Cell::new(0),
        };
        let question = path("Where?").files(&files).root("/r");
        let all = question.suggestions("");
        let only_s = question.suggestions("s");
        let inside = question.suggestions(&format!("src{SEP}"));
        assert_eq!((all.len(), only_s.len(), inside.len()), (3, 1, 2));
        assert_eq!(files.lists.get(), 2);
        assert_eq!(question.suggestions("C").len(), 1);
        assert_eq!(files.lists.get(), 2);
    }

    #[test]
    fn refusals_are_notices_that_messages_word() {
        let files = fake();
        let question = path("Where?")
            .files(&files)
            .root("/r")
            .kind(PathKind::Existing);
        let refusal = |text: &str| {
            let line = TypedAnswer::new(CharFilter::Printable).prefilled(text);
            question.submit(question.state(line, 0))
        };
        assert!(matches!(
            refusal(""),
            Next::Refuse(_, Refusal::Path(PathNotice::Empty))
        ));
        assert!(matches!(
            refusal("nope"),
            Next::Refuse(_, Refusal::Path(PathNotice::Missing(shown))) if shown == "nope"
        ));
    }

    #[test]
    fn a_piped_line_is_the_answer() {
        let files = fake();
        let question = path("Where?").files(&files).root("/r");
        let keys =
            question.typed("Cargo.toml", &crate::render::messages::Messages::english());
        assert_eq!(
            question.simulate(keys).ok(),
            Some(PathBuf::from("/r/Cargo.toml"))
        );
    }

    #[test]
    fn a_missing_file_is_refused_when_it_must_exist() {
        let files = fake();
        let asking = |kind| path("Where?").files(&files).root("/r").kind(kind);
        let missing = asking(PathKind::Existing).simulate(typed("nope.txt", []));
        assert!(
            matches!(&missing, Err(Error::Refused { reason, .. }) if reason == "nope.txt does not exist."),
            "{missing:?}"
        );
        assert_eq!(
            asking(PathKind::Existing)
                .simulate(typed("Cargo.toml", []))
                .ok(),
            Some(PathBuf::from("/r/Cargo.toml"))
        );
        let refused = asking(PathKind::Directory).simulate(typed("Cargo.toml", []));
        assert!(
            matches!(&refused, Err(Error::Refused { reason, .. }) if reason == "Cargo.toml is not a directory."),
            "{refused:?}"
        );
    }

    #[test]
    fn enter_on_nothing_takes_the_default() {
        let files = fake();
        let question = path("Where?").files(&files).default("/r/docs");
        assert_eq!(
            question.simulate([Key::Enter]).ok(),
            Some(PathBuf::from("/r/docs"))
        );
        let bare = path("Where?").files(&files);
        assert!(matches!(
            bare.simulate([Key::Enter]),
            Err(Error::Refused { .. })
        ));
    }

    #[test]
    fn peek_reads_the_path_for_validate() {
        let files = fake();
        let question = path("Where?").files(&files).root("/r");
        let state = question.start().ok();
        assert_eq!(
            state.as_ref().and_then(|s| question.peek(s)),
            Some(Path::new("/r/"))
        );
    }
}
