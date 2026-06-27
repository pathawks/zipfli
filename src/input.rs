use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// A single item to be written into the archive, in the order it should appear.
#[derive(Debug)]
pub enum Entry {
    /// A regular file: read `path` from disk and store it under `name`.
    File { path: PathBuf, name: String },
    /// A directory marker. Its `name` always ends in `/`.
    Directory { name: String },
}

impl Entry {
    /// The name stored in the archive (directories end in `/`).
    pub fn name(&self) -> &str {
        match self {
            Entry::File { name, .. } | Entry::Directory { name } => name,
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Entry::File { .. })
    }
}

/// Build the archive entry name for a path given on the command line,
/// preserving its relative path but normalizing it for the ZIP format:
/// forward slashes, no leading `/`, and `.`/`..`/drive components dropped.
fn archive_name(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Join a directory's archive name with a child name, handling the empty base
/// (e.g. when the input was `.`) so we never emit a leading slash.
fn join_name(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

/// Expand the command-line inputs into a flat, ordered list of archive entries,
/// walking directories recursively. When `include_dirs` is set, an explicit
/// entry is emitted for each directory so that empty directories are preserved.
pub fn expand(inputs: &[PathBuf], include_dirs: bool) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for input in inputs {
        let metadata = fs::metadata(input)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", input.display())))?;
        if metadata.is_dir() {
            let base = archive_name(input);
            if include_dirs && !base.is_empty() {
                entries.push(Entry::Directory {
                    name: format!("{base}/"),
                });
            }
            walk(input, &base, include_dirs, &mut entries)?;
        } else {
            entries.push(Entry::File {
                path: input.clone(),
                name: archive_name(input),
            });
        }
    }
    Ok(entries)
}

/// Recursively collect the contents of `dir`, naming entries relative to
/// `base`. Children are visited in sorted order for deterministic output.
fn walk(dir: &Path, base: &str, include_dirs: bool, entries: &mut Vec<Entry>) -> io::Result<()> {
    let mut children = fs::read_dir(dir)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", dir.display())))?
        .collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|child| child.file_name());

    for child in children {
        let name = join_name(base, &child.file_name().to_string_lossy());
        let file_type = child.file_type()?;
        if file_type.is_dir() {
            if include_dirs {
                entries.push(Entry::Directory {
                    name: format!("{name}/"),
                });
            }
            walk(&child.path(), &name, include_dirs, entries)?;
        } else if file_type.is_file() {
            entries.push(Entry::File {
                path: child.path(),
                name,
            });
        } else {
            // Symlinks and other special files: skip rather than risk a read
            // error or a traversal cycle.
            eprintln!(
                "warning: skipping {} (not a regular file or directory)",
                child.path().display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A temporary directory that is removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("zipfli-test-{}-{n}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| e.name().to_string()).collect()
    }

    /// Create a small tree under a temp dir and return both the temp dir and
    /// the `root/` directory inside it:
    ///   root/file1.txt, root/sub/file2.txt, root/empty/ (empty)
    fn make_tree() -> (TempDir, PathBuf) {
        let temp = TempDir::new();
        let root = temp.0.join("root");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("file1.txt"), b"hello").unwrap();
        fs::write(root.join("sub").join("file2.txt"), b"world").unwrap();
        (temp, root)
    }

    #[test]
    fn archive_name_normalizes_paths() {
        assert_eq!(archive_name(Path::new("a/b/c.txt")), "a/b/c.txt");
        assert_eq!(archive_name(Path::new("./a/b")), "a/b");
        assert_eq!(archive_name(Path::new("/abs/x")), "abs/x");
        assert_eq!(archive_name(Path::new("../up/x")), "up/x");
        assert_eq!(
            archive_name(Path::new("META-INF/container.xml")),
            "META-INF/container.xml"
        );
        assert_eq!(archive_name(Path::new(".")), "");
    }

    #[test]
    fn expand_walks_directory_without_dir_entries() {
        let (_temp, root) = make_tree();
        let base = archive_name(&root);

        let entries = expand(&[root], false).unwrap();

        assert!(entries.iter().all(|e| e.is_file()));
        assert_eq!(
            names(&entries),
            vec![
                format!("{base}/file1.txt"),
                format!("{base}/sub/file2.txt"),
            ],
        );
    }

    #[test]
    fn expand_includes_directory_entries_when_requested() {
        let (_temp, root) = make_tree();
        let base = archive_name(&root);

        let entries = expand(&[root], true).unwrap();

        assert_eq!(
            names(&entries),
            vec![
                format!("{base}/"),
                format!("{base}/empty/"),
                format!("{base}/file1.txt"),
                format!("{base}/sub/"),
                format!("{base}/sub/file2.txt"),
            ],
        );
        assert_eq!(entries.iter().filter(|e| !e.is_file()).count(), 3);
    }

    #[test]
    fn expand_keeps_a_plain_files_relative_path() {
        let (_temp, root) = make_tree();
        let file = root.join("file1.txt");

        let entries = expand(std::slice::from_ref(&file), false).unwrap();

        assert_eq!(names(&entries), vec![archive_name(&file)]);
        assert!(entries[0].is_file());
    }

    #[test]
    fn expand_reports_a_missing_input() {
        let err = expand(&[PathBuf::from("/no/such/path/zzz")], false).unwrap_err();
        assert!(err.to_string().contains("/no/such/path/zzz"));
    }
}
