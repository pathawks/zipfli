# zipfli

Create optimized ZIP archives using [Zopfli](https://github.com/google/zopfli) compression.

zipfli produces smaller ZIP files by using Zopfli's deflate implementation, which achieves better compression ratios than standard deflate at the cost of longer compression times. It includes format presets for EPUB, ODF, and OOXML that automatically handle which files must be stored uncompressed per each format's spec.

## Installation

```sh
cargo install zipfli
```

Or build from source:

```sh
git clone https://github.com/pathawks/zipfli.git
cd zipfli
cargo install --path .
```

## Usage

```sh
zipfli output.zip file1.txt file2.txt
```

Write to stdout:

```sh
zipfli - file1.txt file2.txt > output.zip
```

Add a directory to recurse into it:

```sh
zipfli archive.zip src README.md
```

Use a format preset:

```sh
zipfli --format epub book.epub mimetype META-INF/container.xml content.opf chapter1.xhtml
```

Format presets match entry names against exact paths (`mimetype`, `META-INF/container.xml`, …), and entry names preserve the paths you pass. So run zipfli from the content root — for example, to package an unzipped EPUB directory:

```sh
cd book && zipfli --format epub ../book.epub .
```

### Directories

When an input is a directory, zipfli adds its contents recursively. Entry names keep their path relative to how you named the directory on the command line, so `zipfli archive.zip src` stores `src/main.rs`, `src/zip.rs`, and so on. The same path preservation applies to files given with a path: `META-INF/container.xml` is stored at `META-INF/container.xml`, not flattened to `container.xml`.

Like `zip`, zipfli writes an entry for each directory by default, which preserves empty directories. Pass `--no-dir-entries` to store only regular files and omit the directory markers.

Symlinks given directly on the command line are followed, like any other explicit input; symlinks encountered *inside* a walked directory are skipped with a warning, to avoid traversal cycles. File names are stored as UTF-8.

Inputs that resolve to the same entry (the same file or directory listed twice, or a file listed both on its own and via its parent directory) are added only once. Two *different* files that would collide on the same entry name are an error.

Pass `-j`/`--flatten` (alias `--junk-paths`, like `zip -j`) to store every file under its base name, discarding directory structure and directory entries. Flattening cannot be combined with `--format`, because every format preset requires nested paths (`META-INF/`, `_rels/`).

zipfli writes standard (non-ZIP64) archives, so it supports up to 65,534 entries and a total size just under 4 GiB. Beyond those limits it exits with an error rather than produce a malformed archive.

### Format Presets

| Preset | Stored (uncompressed) files |
|--------|----------------------------|
| `epub` | `mimetype`, `META-INF/manifest.xml`, `META-INF/container.xml` |
| `odf` | `mimetype`, `META-INF/manifest.xml` |
| `ooxml` | `[Content_Types].xml`, `_rels/.rels` |

EPUB and ODF presets also sort `mimetype` to appear first in the archive, as required by their specifications.

If a compressed file ends up larger than the original, it is automatically stored uncompressed instead.

### Options

| Option | Description |
|--------|-------------|
| `-f`, `--format <FORMAT>` | Archive format preset (`epub`, `odf`, `ooxml`) |
| `-i`, `--iterations-without-improvement <N>` | Zopfli iterations without improvement (default: 15) |
| `--no-dir-entries` | Omit directory entries; store only regular files (also drops empty directories) |
| `-j`, `--flatten` | Store every file under its base name, discarding directory structure (alias: `--junk-paths`) |
| `-q`, `--quiet` | Suppress progress output |
| `-h`, `--help` | Print help |
| `-V`, `--version` | Print version |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
