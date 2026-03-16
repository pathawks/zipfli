use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

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

    /// Suppress progress output
    #[arg(short, long)]
    quiet: bool,
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

    let mut inputs = args.inputs;
    if mimetype_first {
        inputs.sort_by_key(|p| p.file_name().unwrap().to_string_lossy() != "mimetype");
    }

    let dt = DosDateTime::zero();
    let total_files = inputs.len();
    let mut reporter = progress::ProgressReporter::new(args.quiet);

    let mut zip_data = vec![];
    let mut central_dir = Vec::new();

    for (index, input) in inputs.iter().enumerate() {
        let filename = input.file_name().unwrap().to_string_lossy().to_string();

        let file_data = match fs::read(input) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Failed to read file {:?}: {}", input, e);
                std::process::exit(1);
            }
        };

        let mut is_store_only = store_only_files.iter().any(|f| filename == *f);

        reporter.start_file(&filename, index, total_files);
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
                eprintln!("Compression failed for file {:?}: {}", input, e);
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
        let compressed_size = compressed.len() as u32;
        let uncompressed_size = file_data.len() as u32;
        let local_header_offset = zip_data.len() as u32;

        let local_header = zip::LocalFileHeader::new(
            &filename,
            compressed_size,
            uncompressed_size,
            crc32,
            dt,
            is_store_only,
        );
        zip_data.extend_from_slice(&local_header.to_bytes());
        zip_data.extend_from_slice(&compressed);

        let central_header = zip::CentralDirectoryHeader::new(
            &filename,
            compressed_size,
            uncompressed_size,
            crc32,
            dt,
            is_store_only,
            local_header_offset,
        );
        central_dir.extend_from_slice(&central_header.to_bytes());
    }

    let offset = zip_data.len() as u32;
    zip_data.extend_from_slice(&central_dir);

    let eocd =
        zip::EndOfCentralDirectory::new(inputs.len() as u16, offset, central_dir.len() as u32);
    zip_data.extend_from_slice(&eocd.to_bytes());

    reporter.finish();

    let output_file: &mut dyn Write = match args.output.as_os_str().to_str() {
        Some("-") => &mut std::io::stdout(),
        _ => &mut File::create(args.output)?,
    };
    output_file.write_all(&zip_data)?;

    Ok(())
}
