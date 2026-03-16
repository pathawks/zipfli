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

Use a format preset:

```sh
zipfli --format epub book.epub mimetype META-INF/container.xml content.opf chapter1.xhtml
```

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
| `-q`, `--quiet` | Suppress progress output |
| `-h`, `--help` | Print help |
| `-V`, `--version` | Print version |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
