//! Path prompt core: listing a directory and suggesting completions for typed
//! text.
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::OsString,
    io,
    path::{MAIN_SEPARATOR, Path, PathBuf},
};

use crate::render::messages::Messages;

/// Why a path is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathNotice {
    /// Nothing was typed.
    Empty,
    /// The path, as typed, does not exist.
    Missing(String),
    /// The path, as typed, is not a directory.
    NotDirectory(String),
}

impl PathNotice {
    /// The words of `messages` for the notice.
    #[must_use]
    pub fn text(&self, messages: &Messages) -> String {
        match self {
            Self::Empty => messages.path_empty.to_string(),
            Self::Missing(path) => (messages.path_missing)(path),
            Self::NotDirectory(path) => (messages.path_not_directory)(path),
        }
    }
}

/// What a directory entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// A file.
    File,
    /// A directory.
    Directory,
    /// Anything else.
    Other,
}

/// One entry of a listed directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    /// The file name.
    pub name: OsString,
    /// What it is.
    pub kind: EntryKind,
}

/// Which entries the prompt accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// Whatever is typed.
    Any,
    /// A file or directory that exists.
    Existing,
    /// A directory that exists.
    Directory,
}

/// Source of directory listings.
pub trait Files: std::fmt::Debug {
    /// Lists the entries of `dir`.
    ///
    /// # Errors
    /// Fails when `dir` is missing or unreadable.
    fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>>;

    /// What `path` is, `None` when it does not exist.
    fn kind(&self, path: &Path) -> Option<EntryKind> {
        match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => self
                .list(parent)
                .ok()?
                .into_iter()
                .find(|entry| entry.name == name)
                .map(|entry| entry.kind),
            _ => self.list(path).ok().map(|_| EntryKind::Directory),
        }
    }
}

/// The directories listed so far in one prompt run, so that a key does not list
/// a directory again.
#[derive(Debug, Default)]
pub(crate) struct Listings {
    seen: RefCell<HashMap<PathBuf, Result<Vec<DirEntry>, io::ErrorKind>>>,
}

impl Listings {
    pub(crate) const fn of<'a>(&'a self, files: &'a dyn Files) -> Cached<'a> {
        Cached {
            files,
            listings: self,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Cached<'a> {
    files: &'a dyn Files,
    listings: &'a Listings,
}

impl Files for Cached<'_> {
    fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>> {
        let mut seen = self.listings.seen.borrow_mut();
        let listed = seen
            .entry(dir.to_path_buf())
            .or_insert_with(|| self.files.list(dir).map_err(|error| error.kind()));
        match listed {
            Ok(entries) => Ok(entries.clone()),
            Err(kind) => Err((*kind).into()),
        }
    }
}

/// The real file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct StdFiles;

impl Files for StdFiles {
    fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let kind = match std::fs::metadata(entry.path()) {
                Ok(meta) if meta.is_dir() => EntryKind::Directory,
                Ok(meta) if meta.is_file() => EntryKind::File,
                _ => EntryKind::Other,
            };
            out.push(DirEntry {
                name: entry.file_name(),
                kind,
            });
        }
        Ok(out)
    }

    /// One `stat`, so that the file system decides what a name matches: its
    /// case and Unicode form are not the prompt's to judge.
    fn kind(&self, path: &Path) -> Option<EntryKind> {
        let meta = std::fs::metadata(path).ok()?;
        Some(if meta.is_dir() {
            EntryKind::Directory
        } else if meta.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        })
    }
}

/// In-memory file system built from a list of paths.
#[cfg(any(test, feature = "fake"))]
#[derive(Clone, Debug, Default)]
pub struct FakeFiles {
    dirs: std::collections::BTreeMap<PathBuf, Vec<DirEntry>>,
}

#[cfg(any(test, feature = "fake"))]
impl FakeFiles {
    /// A fake file system holding `paths`; a path ending in `/` is a directory.
    #[must_use]
    pub fn new(paths: &[&str]) -> Self {
        let mut fake = Self::default();
        for raw in paths {
            let kind = if raw.ends_with('/') {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            let path = Path::new(raw);
            fake.add(path, kind);
            for parent in path.ancestors().skip(1) {
                fake.add(parent, EntryKind::Directory);
            }
        }
        fake
    }

    fn add(&mut self, path: &Path, kind: EntryKind) {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        let list = self.dirs.entry(parent.to_path_buf()).or_default();
        if !list.iter().any(|e| e.name == name) {
            list.push(DirEntry {
                name: name.to_owned(),
                kind,
            });
        }
        if kind == EntryKind::Directory {
            self.dirs.entry(path.to_path_buf()).or_default();
        }
    }
}

#[cfg(any(test, feature = "fake"))]
impl Files for FakeFiles {
    fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>> {
        self.dirs
            .get(dir)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
}

pub(crate) fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

fn split(typed: &str) -> (&str, &str) {
    typed
        .rfind(is_separator)
        .map_or(("", typed), |i| typed.split_at(i + 1))
}

fn starts_with(name: &str, prefix: &str) -> bool {
    if cfg!(windows) {
        name.to_lowercase().starts_with(&prefix.to_lowercase())
    } else {
        name.starts_with(prefix)
    }
}

/// Candidate completions for `typed`: names (directories end with the
/// separator) of the entries in the typed directory that start with the typed
/// prefix. Directories come first, then by name; an unreadable directory gives
/// no candidates.
pub fn suggest(
    typed: &str,
    root: &Path,
    kind: PathKind,
    files: &(impl Files + ?Sized),
) -> Vec<String> {
    let (dir, prefix) = split(typed);
    let dir = if dir.is_empty() {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    let Ok(entries) = files.list(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<(bool, String)> = entries
        .into_iter()
        .filter(|e| kind != PathKind::Directory || e.kind == EntryKind::Directory)
        .filter_map(|e| {
            let name = e.name.to_string_lossy().into_owned();
            let visible = !name.starts_with('.') || prefix.starts_with('.');
            (visible && starts_with(&name, prefix))
                .then_some((e.kind == EntryKind::Directory, name))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found
        .into_iter()
        .map(|(is_dir, mut name)| {
            if is_dir {
                name.push(MAIN_SEPARATOR);
            }
            name
        })
        .collect()
}

/// The typed text with its prefix replaced by `candidate`.
#[must_use]
pub fn complete(typed: &str, candidate: &str) -> String {
    let (dir, _) = split(typed);
    format!("{dir}{candidate}")
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use super::*;

    #[test]
    fn the_real_file_system_judges_what_a_path_is() -> io::Result<()> {
        let dir =
            std::env::temp_dir().join(format!("clacky-kind-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("folder"))?;
        std::fs::write(dir.join("README.md"), "")?;
        let kinds = (
            StdFiles.kind(&dir.join("README.md")),
            StdFiles.kind(&dir.join("folder")),
            StdFiles.kind(&dir.join("missing")),
            StdFiles.kind(&dir.join("README.md").join("below")),
            std::fs::metadata(dir.join("readme.md")).is_ok(),
            StdFiles.kind(&dir.join("readme.md")),
        );
        std::fs::remove_dir_all(&dir)?;
        assert_eq!(kinds.0, Some(EntryKind::File));
        assert_eq!(kinds.1, Some(EntryKind::Directory));
        assert_eq!(kinds.2, None);
        assert_eq!(kinds.3, None);
        assert_eq!(kinds.5.is_some(), kinds.4);
        Ok(())
    }

    const SEP: char = MAIN_SEPARATOR;

    fn fake() -> FakeFiles {
        FakeFiles::new(&[
            "/r/src/main.rs",
            "/r/src/lib.rs",
            "/r/src/util/mod.rs",
            "/r/Cargo.toml",
            "/r/docs/",
            "/r/.git/config",
            "/r/.env",
            "/r/ünï/файл.txt",
            "/r/ünï/папка/",
        ])
    }

    fn sug(typed: &str, kind: PathKind) -> Vec<String> {
        suggest(typed, Path::new("/r"), kind, &fake())
    }

    fn dir(name: &str) -> String {
        format!("{name}{SEP}")
    }

    #[test]
    fn root_lists_dirs_first_then_names_hiding_dotfiles() {
        assert_eq!(
            sug("", PathKind::Any),
            vec![dir("docs"), dir("src"), dir("ünï"), "Cargo.toml".to_owned()]
        );
    }

    #[test]
    fn nested_directory() {
        assert_eq!(
            sug("src/", PathKind::Any),
            vec![dir("util"), "lib.rs".to_owned(), "main.rs".to_owned()]
        );
    }

    #[test]
    fn prefix_filter_is_case_sensitive_on_unix() {
        assert_eq!(sug("src/m", PathKind::Any), vec!["main.rs".to_owned()]);
        if cfg!(windows) {
            assert_eq!(sug("src/M", PathKind::Any), vec!["main.rs".to_owned()]);
        } else {
            assert_eq!(sug("src/M", PathKind::Any), Vec::<String>::new());
        }
    }

    #[test]
    fn directory_only() {
        assert_eq!(
            sug("", PathKind::Directory),
            vec![dir("docs"), dir("src"), dir("ünï")]
        );
        assert_eq!(sug("src/", PathKind::Directory), vec![dir("util")]);
    }

    #[test]
    fn dotfiles_need_dot_prefix() {
        assert_eq!(
            sug(".", PathKind::Any),
            vec![dir(".git"), ".env".to_owned()]
        );
        assert_eq!(sug(".e", PathKind::Any), vec![".env".to_owned()]);
    }

    #[test]
    fn missing_dir_is_empty() {
        assert_eq!(sug("nope/", PathKind::Any), Vec::<String>::new());
        assert_eq!(sug("src/main.rs/", PathKind::Any), Vec::<String>::new());
    }

    #[test]
    fn unicode_names() {
        assert_eq!(sug("ü", PathKind::Any), vec![dir("ünï")]);
        assert_eq!(
            sug("ünï/", PathKind::Any),
            vec![dir("папка"), "файл.txt".to_owned()]
        );
        assert_eq!(sug("ünï/ф", PathKind::Any), vec!["файл.txt".to_owned()]);
    }

    #[test]
    fn absolute_input_ignores_root() {
        let found =
            suggest("/r/src/l", Path::new("/elsewhere"), PathKind::Any, &fake());
        assert_eq!(found, vec!["lib.rs".to_owned()]);
    }

    #[test]
    fn complete_replaces_prefix() {
        assert_eq!(complete("src/ma", "main.rs"), "src/main.rs");
        assert_eq!(complete("s", &dir("src")), format!("src{SEP}"));
        assert_eq!(complete("a/b/", "c"), "a/b/c");
    }
}
