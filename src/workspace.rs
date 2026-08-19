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

        let path = path.unwrap_or(std::env::current_dir()?);
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

fn is_markdown(path: &Path) -> bool {
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
    use std::path::{Path, PathBuf};

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
}
