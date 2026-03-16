use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const BRAILLE_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn savings_pct(original: u64, compressed: u64) -> u64 {
    if original == 0 {
        return 0;
    }
    ((original - compressed) as f64 / original as f64 * 100.0).round() as u64
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub struct FileResult {
    pub name: String,
    pub original_size: u64,
    pub compressed_size: u64,
    pub stored: bool,
    pub elapsed: Duration,
}

struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    fn start(name: String, index: usize, total: usize, use_color: bool) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();

        let handle = thread::spawn(move || {
            let mut stderr = std::io::stderr().lock();
            let mut frame = 0usize;
            while !stop_clone.load(Ordering::Relaxed) {
                let ch = BRAILLE_FRAMES[frame % BRAILLE_FRAMES.len()];
                if use_color {
                    let _ = write!(
                        stderr,
                        "\r\x1b[K  \x1b[36m{ch}\x1b[0m {name}  [{}/{}]",
                        index + 1,
                        total
                    );
                } else {
                    let _ = write!(
                        stderr,
                        "\r\x1b[K  {ch} {name}  [{}/{}]",
                        index + 1,
                        total
                    );
                }
                let _ = stderr.flush();
                frame += 1;
                thread::sleep(Duration::from_millis(80));
            }
            let _ = write!(stderr, "\r\x1b[K");
            let _ = stderr.flush();
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

pub struct ProgressReporter {
    quiet: bool,
    is_tty: bool,
    use_color: bool,
    spinner: Option<Spinner>,
    total_original: u64,
    total_compressed: u64,
    file_count: usize,
    total_elapsed: Duration,
}

impl ProgressReporter {
    pub fn new(quiet: bool) -> Self {
        let is_tty = std::io::stderr().is_terminal();
        let use_color = is_tty && std::env::var_os("NO_COLOR").is_none();
        Self {
            quiet,
            is_tty,
            use_color,
            spinner: None,
            total_original: 0,
            total_compressed: 0,
            file_count: 0,
            total_elapsed: Duration::ZERO,
        }
    }

    pub fn start_file(&mut self, name: &str, index: usize, total: usize) {
        if self.quiet || !self.is_tty {
            return;
        }
        self.spinner = Some(Spinner::start(
            name.to_string(),
            index,
            total,
            self.use_color,
        ));
    }

    pub fn finish_file(&mut self, result: FileResult) {
        if let Some(spinner) = self.spinner.take() {
            spinner.stop();
        }

        self.total_original += result.original_size;
        self.total_compressed += result.compressed_size;
        self.file_count += 1;
        self.total_elapsed += result.elapsed;

        if self.quiet {
            return;
        }

        let mut stderr = std::io::stderr().lock();

        if self.is_tty {
            if result.stored {
                let size = format_size(result.original_size);
                if self.use_color {
                    let _ = writeln!(
                        stderr,
                        "  \x1b[2m·\x1b[0m {:<24} \x1b[2m{size}  (stored)\x1b[0m",
                        result.name
                    );
                } else {
                    let _ = writeln!(
                        stderr,
                        "  · {:<24} {size}  (stored)",
                        result.name
                    );
                }
            } else {
                let orig = format_size(result.original_size);
                let comp = format_size(result.compressed_size);
                let pct = savings_pct(result.original_size, result.compressed_size);
                let secs = result.elapsed.as_secs_f64();
                if self.use_color {
                    let _ = writeln!(
                        stderr,
                        "  \x1b[32m✓\x1b[0m {:<24} {orig} → {comp}  ({pct}%)  {secs:.1}s",
                        result.name
                    );
                } else {
                    let _ = writeln!(
                        stderr,
                        "  ✓ {:<24} {orig} → {comp}  ({pct}%)  {secs:.1}s",
                        result.name
                    );
                }
            }
        } else {
            // Non-TTY: no ANSI, ASCII arrow
            if result.stored {
                let size = format_size(result.original_size);
                let _ = writeln!(stderr, "{}  {size}  (stored)", result.name);
            } else {
                let orig = format_size(result.original_size);
                let comp = format_size(result.compressed_size);
                let pct = if result.original_size > 0 {
                    100 - (result.compressed_size * 100 / result.original_size)
                } else {
                    0
                };
                let secs = result.elapsed.as_secs_f64();
                let _ = writeln!(
                    stderr,
                    "{}  {orig} -> {comp}  ({pct}%)  {secs:.1}s",
                    result.name
                );
            }
        }
    }

    pub fn finish(&mut self) {
        if self.quiet {
            return;
        }

        let mut stderr = std::io::stderr().lock();

        let orig = format_size(self.total_original);
        let comp = format_size(self.total_compressed);
        let pct = savings_pct(self.total_original, self.total_compressed);
        let secs = self.total_elapsed.as_secs_f64();
        let n = self.file_count;
        let files_word = if n == 1 { "file" } else { "files" };

        if self.is_tty {
            if self.use_color {
                let _ = writeln!(
                    stderr,
                    "  \x1b[1m{n} {files_word}\x1b[0m  {orig} → {comp}  ({pct}%)  {secs:.1}s"
                );
            } else {
                let _ =
                    writeln!(stderr, "  {n} {files_word}  {orig} → {comp}  ({pct}%)  {secs:.1}s");
            }
        } else {
            let _ =
                writeln!(stderr, "{n} {files_word}  {orig} -> {comp}  ({pct}%)  {secs:.1}s");
        }
    }
}
