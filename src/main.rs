use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

mod input;
mod progress;
mod zip;

#[derive(Debug, Clone, Copy)]
struct DosDateTime {
    time: u16,
    date: u16,
}

impl DosDateTime {
    fn zero() -> Self {
        Self { time: 0, date: 0 }
    }
}

#[derive(Clone, Debug, clap::ValueEnum)]
enum Format {
    Epub,
    Odf,
    Ooxml,
}

impl Format {
    fn store_only_files(&self) -> &'static [&'static str] {
        match self {
            Self::Epub => &["mimetype", "META-INF/manifest.xml", "META-INF/container.xml"],
            Self::Odf => &["mimetype", "META-INF/manifest.xml"],
            Self::Ooxml => &["[Content_Types].xml", "_rels/.rels"],
        }
    }

    fn mimetype_first(&self) -> bool {
        matches!(self, Self::Epub | Self::Odf)
    }
}

#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Output file (use '-' for stdout)
    #[arg(required = true)]
    output: PathBuf,

    /// Input files
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

    /// Archive format preset (controls which files are stored without compression)
    #[arg(short, long)]
    format: Option<Format>,

    /// Number of iterations without improvement
    #[arg(
        short = 'i',
        long = "iterations-without-improvement",
        default_value = "15"
    )]
    iterations_without_improvement: std::num::NonZeroU64,

    /// Do not store entries for directories themselves (their contents are
    /// still included; this also drops empty directories)
    #[arg(long = "no-dir-entries", action = clap::ArgAction::SetFalse)]
    dir_entries: bool,

    /// Flatten paths: store every file under its base name, discarding
    /// directory structure (like zip's -j; entry names must stay unique)
    #[arg(
        short = 'j',
        long,
        visible_alias = "junk-paths",
        conflicts_with = "format"
    )]
    flatten: bool,

    /// Suppress progress output
    #[arg(short, long)]
    quiet: bool,
}

/// Convert an archive length to the u32 field a standard ZIP requires,
/// exiting with a clear error when the value would need ZIP64. The sentinel
/// 0xFFFFFFFF itself is rejected too: readers reserve it for "see the ZIP64
/// record", which we never write.
fn require_u32(len: usize, what: &str) -> u32 {
    if len >= u32::MAX as usize {
        eprintln!("error: {what} exceeds the 4 GiB standard ZIP limit (ZIP64 is not supported)");
        std::process::exit(1);
    }
    len as u32
}

fn main() -> std::io::Result<()> {
    let args = <Cli as clap::Parser>::parse();

    let options = zopfli::Options {
        iteration_count: std::num::NonZeroU64::MAX,
        iterations_without_improvement: args.iterations_without_improvement,
        maximum_block_splits: u16::MAX,
    };

    let store_only_files: &[&str] = args
        .format
        .as_ref()
        .map_or(&[], |f| f.store_only_files());

    let mimetype_first = args.format.as_ref().is_some_and(|f| f.mimetype_first());

    let expand_options = input::Options {
        dir_entries: args.dir_entries,
        flatten: args.flatten,
    };
    let mut entries = match input::expand(&args.inputs, expand_options) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    if mimetype_first {
        entries.sort_by_key(|entry| entry.name() != "mimetype");
    }

    // The 0xFFFF entry count is the ZIP64 sentinel, so stop one short of it.
    if entries.len() >= u16::MAX as usize {
        eprintln!(
            "error: archive has {} entries; a standard ZIP holds at most {} (ZIP64 is not supported)",
            entries.len(),
            u16::MAX - 1
        );
        std::process::exit(1);
    }

    let dt = DosDateTime::zero();
    let total_files = entries.iter().filter(|entry| entry.is_file()).count();
    let mut reporter = progress::ProgressReporter::new(args.quiet);

    let mut zip_data = vec![];
    let mut central_dir = Vec::new();
    let mut file_index = 0usize;

    for entry in &entries {
        let local_header_offset = require_u32(zip_data.len(), "archive");

        let (filename, path) = match entry {
            input::Entry::Directory { name } => {
                zip_data.extend_from_slice(&zip::LocalFileHeader::directory(name).to_bytes());
                central_dir.extend_from_slice(
                    &zip::CentralDirectoryHeader::directory(name, local_header_offset).to_bytes(),
                );
                continue;
            }
            input::Entry::File { name, path } => (name, path),
        };

        let file_data = match fs::read(path) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Failed to read file {:?}: {}", path, e);
                std::process::exit(1);
            }
        };

        let uncompressed_size = require_u32(file_data.len(), &path.display().to_string());

        let mut is_store_only = store_only_files.contains(&filename.as_str());

        reporter.start_file(filename, file_index, total_files);
        let start = Instant::now();

        let mut compressed = if is_store_only {
            file_data.clone()
        } else {
            let mut compressed = vec![];
            if let Err(e) = zopfli::compress(
                options,
                zopfli::Format::Deflate,
                file_data.as_slice(),
                &mut compressed,
            ) {
                eprintln!("Compression failed for file {:?}: {}", path, e);
                std::process::exit(1);
            }
            compressed
        };

        if !is_store_only && compressed.len() >= file_data.len() {
            is_store_only = true;
            compressed = file_data.clone();
        }

        let elapsed = start.elapsed();

        reporter.finish_file(progress::FileResult {
            name: filename.clone(),
            original_size: file_data.len() as u64,
            compressed_size: compressed.len() as u64,
            stored: is_store_only,
            elapsed,
        });

        let crc32 = crc32fast::hash(&file_data);
        // The stored fallback caps compressed at file_data's guarded length.
        let compressed_size = compressed.len() as u32;

        let local_header = zip::LocalFileHeader::new(
            filename,
            compressed_size,
            uncompressed_size,
            crc32,
            dt,
            is_store_only,
        );
        zip_data.extend_from_slice(&local_header.to_bytes());
        zip_data.extend_from_slice(&compressed);

        let central_header = zip::CentralDirectoryHeader::new(
            filename,
            compressed_size,
            uncompressed_size,
            crc32,
            dt,
            is_store_only,
            local_header_offset,
        );
        central_dir.extend_from_slice(&central_header.to_bytes());

        file_index += 1;
    }

    let offset = require_u32(zip_data.len(), "archive");
    zip_data.extend_from_slice(&central_dir);

    let eocd = zip::EndOfCentralDirectory::new(
        entries.len() as u16,
        offset,
        require_u32(central_dir.len(), "central directory"),
    );
    zip_data.extend_from_slice(&eocd.to_bytes());

    reporter.finish();

    let output_file: &mut dyn Write = match args.output.as_os_str().to_str() {
        Some("-") => &mut std::io::stdout(),
        _ => &mut File::create(args.output)?,
    };
    output_file.write_all(&zip_data)?;

    Ok(())
}
