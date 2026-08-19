use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::workspace::is_markdown;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Parent,
    Directory,
    Markdown,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activation {
    Navigated,
    OpenMarkdown(PathBuf),
    Unsupported(PathBuf),
}

#[derive(Debug)]
pub struct FileExplorer {
    directory: PathBuf,
    entries: Vec<Entry>,
    selected: usize,
    generation: u64,
}

impl FileExplorer {
    pub fn open(directory: impl AsRef<Path>) -> io::Result<Self> {
        let directory = fs::canonicalize(directory)?;
        let entries = read_entries(&directory)?;
        Ok(Self {
            directory,
            entries,
            selected: 0,
            generation: 0,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1).min(self.entries.len() - 1);
        }
    }

    pub fn activate(&mut self) -> io::Result<Activation> {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return Ok(Activation::Navigated);
        };
        match entry.kind {
            EntryKind::Parent | EntryKind::Directory => {
                self.change_directory(&entry.path)?;
                Ok(Activation::Navigated)
            }
            EntryKind::Markdown => Ok(Activation::OpenMarkdown(entry.path)),
            EntryKind::File => Ok(Activation::Unsupported(entry.path)),
        }
    }

    pub fn go_parent(&mut self) -> io::Result<bool> {
        let Some(parent) = self.directory.parent().map(Path::to_path_buf) else {
            return Ok(false);
        };
        self.change_directory(&parent)?;
        Ok(true)
    }

    pub fn refresh(&mut self) -> io::Result<()> {
        let selected_path = self
            .entries
            .get(self.selected)
            .map(|entry| entry.path.clone());
        self.entries = read_entries(&self.directory)?;
        self.selected = selected_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .unwrap_or_else(|| self.selected.min(self.entries.len().saturating_sub(1)));
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    fn change_directory(&mut self, directory: &Path) -> io::Result<()> {
        let directory = fs::canonicalize(directory)?;
        let entries = read_entries(&directory)?;
        self.directory = directory;
        self.entries = entries;
        self.selected = 0;
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }
}

fn read_entries(directory: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    if let Some(parent) = directory.parent() {
        entries.push(Entry {
            path: parent.to_path_buf(),
            name: "..".to_string(),
            kind: EntryKind::Parent,
        });
    }

    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = match fs::metadata(&path) {
            Ok(metadata) if metadata.is_dir() => EntryKind::Directory,
            Ok(metadata) if metadata.is_file() && is_markdown(&path) => EntryKind::Markdown,
            _ => EntryKind::File,
        };
        entries.push(Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
            kind,
        });
    }

    entries.sort_by_cached_key(|entry| {
        (
            entry_rank(entry.kind),
            entry.name.to_lowercase(),
            entry.name.clone(),
        )
    });
    Ok(entries)
}

fn entry_rank(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::Parent => 0,
        EntryKind::Directory => 1,
        EntryKind::Markdown => 2,
        EntryKind::File => 3,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{Activation, EntryKind, FileExplorer};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("markr-explorer-{}-{id}", std::process::id()));
            fs::create_dir_all(path.join("guides")).expect("fixture directories");
            fs::write(path.join("README.md"), "# fixture").expect("Markdown fixture");
            fs::write(path.join("notes.txt"), "fixture").expect("text fixture");
            Self { path }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).expect("remove fixture");
        }
    }

    #[test]
    fn lists_directories_before_markdown_and_other_files() {
        let fixture = Fixture::new();
        let explorer = FileExplorer::open(&fixture.path).expect("explorer");
        let entries = explorer
            .entries()
            .iter()
            .filter(|entry| entry.kind != EntryKind::Parent)
            .map(|entry| (entry.name.as_str(), entry.kind))
            .collect::<Vec<_>>();

        assert_eq!(
            entries,
            vec![
                ("guides", EntryKind::Directory),
                ("README.md", EntryKind::Markdown),
                ("notes.txt", EntryKind::File),
            ]
        );
    }

    #[test]
    fn navigates_into_a_directory_and_back_to_its_parent() {
        let fixture = Fixture::new();
        let mut explorer = FileExplorer::open(&fixture.path).expect("explorer");
        let directory_index = explorer
            .entries()
            .iter()
            .position(|entry| entry.kind == EntryKind::Directory)
            .expect("directory entry");
        for _ in 0..directory_index {
            explorer.move_down();
        }

        assert_eq!(
            explorer.activate().expect("activate"),
            Activation::Navigated
        );
        assert_eq!(
            explorer.directory(),
            canonical(&fixture.path.join("guides"))
        );
        assert!(explorer.go_parent().expect("parent"));
        assert_eq!(explorer.directory(), canonical(&fixture.path));
    }

    fn canonical(path: &Path) -> PathBuf {
        fs::canonicalize(path).expect("canonical path")
    }
}
