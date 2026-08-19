use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Workspace {
    pub root: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    pub selected: usize,
    pub stdin_content: Option<String>,
}

impl Workspace {
    pub fn open(path: Option<PathBuf>, stdin_is_terminal: bool) -> io::Result<Self> {
        if path.is_none() && !stdin_is_terminal {
            let mut content = String::new();
            io::stdin().read_to_string(&mut content)?;
            return Ok(Self {
                root: None,
                files: Vec::new(),
                selected: 0,
                stdin_content: Some(content),
            });
        }

        let path = fs::canonicalize(path.unwrap_or(std::env::current_dir()?))?;
        let metadata = fs::metadata(&path)?;
        let (root, files) = if metadata.is_file() {
            let root = path.parent().map(Path::to_path_buf);
            (root, vec![path])
        } else {
            let mut files = Vec::new();
            collect_markdown_files(&path, &mut files)?;
            files.sort();
            (Some(path), files)
        };

        if files.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no Markdown files found in the selected workspace",
            ));
        }

        Ok(Self {
            root,
            files,
            selected: 0,
            stdin_content: None,
        })
    }

    pub fn active_path(&self) -> Option<&Path> {
        self.files.get(self.selected).map(PathBuf::as_path)
    }

    pub fn next_file(&mut self) {
        if !self.files.is_empty() {
            self.selected = (self.selected + 1) % self.files.len();
        }
    }

    pub fn previous_file(&mut self) {
        if !self.files.is_empty() {
            self.selected = self.selected.checked_sub(1).unwrap_or(self.files.len() - 1);
        }
    }

    pub fn display_name(&self) -> String {
        self.active_path()
            .map(|path| self.display_path(path))
            .unwrap_or_else(|| "stdin".to_string())
    }

    pub fn display_path(&self, path: &Path) -> String {
        self.root
            .as_deref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .display()
            .to_string()
    }

    pub fn reload_content(&self) -> io::Result<String> {
        match self.active_path() {
            Some(path) => fs::read_to_string(path),
            None => Ok(self.stdin_content.clone().unwrap_or_default()),
        }
    }

    pub fn explorer_start_directory(&self) -> io::Result<PathBuf> {
        self.root.clone().map_or_else(std::env::current_dir, Ok)
    }

    pub fn open_file(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = fs::canonicalize(path)?;
        let metadata = fs::metadata(&path)?;
        if !metadata.is_file() || !is_markdown(&path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the selected file is not a Markdown document",
            ));
        }

        if !self.files.contains(&path) {
            self.files.push(path.clone());
            self.files.sort();
        }
        self.selected = self
            .files
            .iter()
            .position(|candidate| candidate == &path)
            .expect("opened file is part of the workspace");
        self.stdin_content = None;
        Ok(())
    }
}

fn collect_markdown_files(path: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_markdown_files(&entry_path, files)?;
        } else if metadata.is_file() && is_markdown(&entry_path) {
            files.push(entry_path);
        }
    }
    Ok(())
}

pub(crate) fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{Workspace, is_markdown};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("markr-workspace-{}-{id}", std::process::id()));
            fs::create_dir_all(path.join("first")).expect("first directory");
            fs::create_dir_all(path.join("second")).expect("second directory");
            fs::write(path.join("first/one.md"), "# One").expect("first document");
            fs::write(path.join("second/two.md"), "# Two").expect("second document");
            Self { path }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).expect("remove fixture");
        }
    }

    #[test]
    fn recognizes_markdown_extensions() {
        assert!(is_markdown(Path::new("guide.md")));
        assert!(is_markdown(Path::new("guide.MARKDOWN")));
        assert!(!is_markdown(Path::new("guide.txt")));
    }

    #[test]
    fn displays_paths_relative_to_the_workspace_root() {
        let workspace = Workspace {
            root: Some(PathBuf::from("/workspace")),
            files: Vec::new(),
            selected: 0,
            stdin_content: None,
        };

        assert_eq!(
            workspace.display_path(Path::new("/workspace/guides/start.md")),
            "guides/start.md"
        );
    }

    #[test]
    fn opens_markdown_documents_outside_the_initial_root() {
        let fixture = Fixture::new();
        let first = fixture.path.join("first/one.md");
        let second = fixture.path.join("second/two.md");
        let mut workspace = Workspace::open(Some(first), true).expect("workspace");

        workspace.open_file(&second).expect("open second document");

        assert_eq!(workspace.files.len(), 2);
        assert_eq!(
            workspace.active_path(),
            Some(
                fs::canonicalize(second)
                    .expect("canonical second")
                    .as_path()
            )
        );
        assert_eq!(workspace.reload_content().expect("content"), "# Two");
    }
}
