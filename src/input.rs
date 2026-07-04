use std::collections::HashMap;
use std::collections::hash_map::Entry as MapEntry;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// How command-line inputs are expanded into archive entries.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Emit an entry for each directory (preserves empty directories).
    pub dir_entries: bool,
    /// Store every file under its base name, discarding directory structure.
    pub flatten: bool,
}

impl Options {
    /// Directory markers are only meaningful when structure is kept.
    fn include_dirs(self) -> bool {
        self.dir_entries && !self.flatten
    }
}

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

/// The final segment of an archive name, used when flattening.
fn basename(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// Attach the offending path to an I/O error so the user sees which input
/// failed.
fn path_err(path: &Path, e: io::Error) -> io::Error {
    io::Error::new(e.kind(), format!("{}: {e}", path.display()))
}

/// Expand the command-line inputs into a flat, ordered, de-duplicated list of
/// archive entries, walking directories recursively. Explicit inputs are
/// followed even when they are symlinks; symlinks encountered *inside* a
/// walked directory are skipped.
pub fn expand(inputs: &[PathBuf], options: Options) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for input in inputs {
        let metadata = fs::metadata(input).map_err(|e| path_err(input, e))?;
        if metadata.is_dir() {
            let base = archive_name(input);
            if options.include_dirs() && !base.is_empty() {
                entries.push(Entry::Directory {
                    name: format!("{base}/"),
                });
            }
            walk(input, &base, options, &mut entries)?;
        } else {
            let full = archive_name(input);
            let name = if options.flatten {
                basename(&full).to_string()
            } else {
                full
            };
            entries.push(Entry::File {
                path: input.clone(),
                name,
            });
        }
    }
    dedup(entries)
}

/// Recursively collect the contents of `dir`, naming entries relative to
/// `base`. Children are visited in sorted order for deterministic output.
fn walk(dir: &Path, base: &str, options: Options, entries: &mut Vec<Entry>) -> io::Result<()> {
    let mut children = fs::read_dir(dir)
        .map_err(|e| path_err(dir, e))?
        .collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|child| child.file_name());

    for child in children {
        let full = join_name(base, &child.file_name().to_string_lossy());
        let file_type = child.file_type()?;
        if file_type.is_dir() {
            if options.include_dirs() {
                entries.push(Entry::Directory {
                    name: format!("{full}/"),
                });
            }
            walk(&child.path(), &full, options, entries)?;
        } else if file_type.is_file() {
            let name = if options.flatten {
                basename(&full).to_string()
            } else {
                full
            };
            entries.push(Entry::File {
                path: child.path(),
                name,
            });
        } else {
            // Symlinks and other special files inside a walked directory:
            // skip rather than risk a read error or a traversal cycle.
            eprintln!(
                "warning: skipping {} (not a regular file or directory)",
                child.path().display()
            );
        }
    }
    Ok(())
}

/// Whether two paths refer to the same file, resolving symlinks and
/// relative-path spellings. Only consulted when two entries collide on the
/// same archive name, so the extra stat cost is off the common path.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// Drop entries that repeat an archive name already produced by the same
/// source (the same directory or file listed twice); reject a name produced
/// by two *different* files, since one would silently overwrite the other.
fn dedup(entries: Vec<Entry>) -> io::Result<Vec<Entry>> {
    let mut seen: HashMap<String, Option<PathBuf>> = HashMap::new();
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        let source = match &entry {
            Entry::File { path, .. } => Some(path.clone()),
            Entry::Directory { .. } => None,
        };
        match seen.entry(entry.name().to_string()) {
            MapEntry::Vacant(slot) => {
                slot.insert(source);
                result.push(entry);
            }
            MapEntry::Occupied(slot) => match (slot.get(), &source) {
                // The same directory marker or the same file seen again.
                (None, None) => {}
                (Some(prev), Some(cur)) => {
                    if !same_file(prev, cur) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!(
                                "duplicate entry name {:?}: from both {} and {}",
                                entry.name(),
                                prev.display(),
                                cur.display()
                            ),
                        ));
                    }
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "duplicate entry name {:?}: two different inputs map to it",
                            entry.name()
                        ),
                    ));
                }
            },
        }
    }
    Ok(result)
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

        let entries = expand(&[root], Options::default()).unwrap();

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
        let options = Options {
            dir_entries: true,
            ..Options::default()
        };

        let entries = expand(&[root], options).unwrap();

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

        let entries = expand(std::slice::from_ref(&file), Options::default()).unwrap();

        assert_eq!(names(&entries), vec![archive_name(&file)]);
        assert!(entries[0].is_file());
    }

    #[test]
    fn expand_reports_a_missing_input() {
        let missing = std::env::temp_dir().join("zipfli-test-definitely-missing");
        let err = expand(std::slice::from_ref(&missing), Options::default()).unwrap_err();
        assert!(err.to_string().contains(&missing.display().to_string()));
    }

    #[test]
    fn flatten_stores_basenames_and_no_directory_markers() {
        let (_temp, root) = make_tree();
        let options = Options {
            flatten: true,
            dir_entries: true, // must be ignored when flattening
        };

        let entries = expand(&[root], options).unwrap();

        assert_eq!(names(&entries), vec!["file1.txt", "file2.txt"]);
        assert!(entries.iter().all(|e| e.is_file()));
    }

    #[test]
    fn flatten_rejects_colliding_names_from_different_files() {
        let (_temp, root) = make_tree();
        fs::write(root.join("sub").join("file1.txt"), b"impostor").unwrap();
        let options = Options {
            flatten: true,
            ..Options::default()
        };

        let err = expand(&[root], options).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("file1.txt"));
    }

    #[test]
    fn repeated_and_overlapping_inputs_are_deduplicated() {
        let (_temp, root) = make_tree();
        let file = root.join("file1.txt");
        let options = Options {
            dir_entries: true,
            ..Options::default()
        };

        let twice = expand(&[root.clone(), file, root.clone()], options).unwrap();
        let once = expand(&[root], options).unwrap();

        assert_eq!(names(&twice), names(&once));
    }
}
