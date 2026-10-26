//! Logging: prints to the board's serial console (if it has one), and keeps the
//! most recent output in RAM so clients can fetch it over the control link (LOGS).

use core::cell::RefCell;
use core::fmt::Write as _;

use critical_section::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use log::{Level, LevelFilter, Log, Metadata, Record};
use static_cell::StaticCell;

/// Bytes of recent log output kept for LOGS.
const CAPACITY: usize = 8 * 1024;
/// Longest log line kept (longer lines are cut, ending in "…").
const LINE_MAX: usize = 240;
/// Log filter at build time, same syntax as esp-println: `level` and `target=level`,
/// comma-separated. From `NXC_LOG`, else `ESP_LOG` (what esp-println reads), else info.
const FILTER: &str = match (option_env!("NXC_LOG"), option_env!("ESP_LOG")) {
    (Some(filter), _) | (None, Some(filter)) => filter,
    (None, None) => "info",
};

struct Ring {
    buf: [u8; CAPACITY],
    /// Bytes ever written. The buffer holds absolute offsets `total - len .. total`.
    total: u64,
    len: usize,
}

impl Ring {
    fn push(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.buf[(self.total % CAPACITY as u64) as usize] = b;
            self.total += 1;
            self.len = (self.len + 1).min(CAPACITY);
        }
    }

    fn oldest(&self) -> u64 {
        self.total - self.len as u64
    }
}

static RING: Mutex<RefCell<Ring>> = Mutex::new(RefCell::new(Ring {
    buf: [0; CAPACITY],
    total: 0,
    len: 0,
}));

/// Who is reading: each transport waits on its own signal, so new output wakes both.
#[derive(Clone, Copy)]
pub enum Reader {
    Tcp = 0,
    Ble = 1,
}

static NEW_OUTPUT: [Signal<CriticalSectionRawMutex, ()>; 2] = [Signal::new(), Signal::new()];

/// A reader's position in the log, as an absolute byte offset.
#[derive(Clone, Copy, Default)]
pub struct Cursor(u64);

impl Cursor {
    /// The oldest output still in memory.
    pub fn oldest() -> Self {
        critical_section::with(|cs| Cursor(RING.borrow_ref(cs).oldest()))
    }

    /// Whether output was dropped before the oldest byte still in memory.
    pub fn anything_dropped() -> bool {
        critical_section::with(|cs| RING.borrow_ref(cs).oldest() > 0)
    }

    /// Whether there's output this cursor hasn't read yet.
    pub fn has_unread(&self) -> bool {
        critical_section::with(|cs| RING.borrow_ref(cs).total > self.0)
    }

    /// Copies unread output into `out` and advances. Returns the byte count and
    /// whether output was lost since the last read (the reader fell behind).
    pub fn read(&mut self, out: &mut [u8]) -> (usize, bool) {
        critical_section::with(|cs| {
            let ring = RING.borrow_ref(cs);
            let lost = self.0 < ring.oldest();
            if lost {
                self.0 = ring.oldest();
            }
            let n = ((ring.total - self.0) as usize).min(out.len());
            for (i, slot) in out[..n].iter_mut().enumerate() {
                *slot = ring.buf[((self.0 + i as u64) % CAPACITY as u64) as usize];
            }
            self.0 += n as u64;
            (n, lost)
        })
    }
}

/// Waits until new output is logged after the last call for this reader.
pub async fn wait_for_output(reader: Reader) {
    NEW_OUTPUT[reader as usize].wait().await;
}

/// Installs the logger, printing to `console` too if the board has one. Call
/// once, first thing at boot.
pub(crate) fn init(console: Option<fn(core::fmt::Arguments<'_>)>) {
    static LOGGER: StaticCell<BufferLogger> = StaticCell::new();
    let logger: &'static BufferLogger = LOGGER.init(BufferLogger { console });
    // SAFETY: called once at boot, before anything else logs (as esp-println does).
    unsafe {
        let _ = log::set_logger_racy(logger);
        log::set_max_level_racy(max_level());
    }
}

struct BufferLogger {
    console: Option<fn(core::fmt::Arguments<'_>)>,
}

impl Log for BufferLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= level_for(metadata.target())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut line = Line::default();
        let _ = write!(line, "{} - {}", record.level(), record.args());
        let text = line.finish();

        if let Some(print) = self.console {
            let color = match record.level() {
                Level::Error => "\u{1b}[31m",
                Level::Warn => "\u{1b}[33m",
                Level::Info => "\u{1b}[32m",
                Level::Debug => "\u{1b}[34m",
                Level::Trace => "\u{1b}[35m",
            };
            // Without the trailing newline: the board's println adds one.
            print(format_args!("{color}{}\u{1b}[0m", text.trim_end_matches('\n')));
        }

        critical_section::with(|cs| RING.borrow_ref_mut(cs).push(text.as_bytes()));
        for signal in &NEW_OUTPUT {
            signal.signal(());
        }
    }

    fn flush(&self) {}
}

/// One formatted log line, cut at [`LINE_MAX`] on a character boundary.
struct Line {
    buf: [u8; LINE_MAX + 4],
    len: usize,
    cut: bool,
}

impl Default for Line {
    fn default() -> Self {
        Self {
            buf: [0; LINE_MAX + 4],
            len: 0,
            cut: false,
        }
    }
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for ch in s.chars() {
            let mut utf8 = [0; 4];
            let bytes = ch.encode_utf8(&mut utf8).as_bytes();
            if self.len + bytes.len() > LINE_MAX {
                self.cut = true;
                return Ok(());
            }
            self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
        }
        Ok(())
    }
}

impl Line {
    fn finish(&mut self) -> &str {
        let tail: &[u8] = if self.cut { "…\n".as_bytes() } else { b"\n" };
        self.buf[self.len..self.len + tail.len()].copy_from_slice(tail);
        self.len += tail.len();
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("?\n")
    }
}

fn parse_level(s: &str) -> Option<LevelFilter> {
    Some(match s.trim() {
        s if s.eq_ignore_ascii_case("off") => LevelFilter::Off,
        s if s.eq_ignore_ascii_case("error") => LevelFilter::Error,
        s if s.eq_ignore_ascii_case("warn") => LevelFilter::Warn,
        s if s.eq_ignore_ascii_case("info") => LevelFilter::Info,
        s if s.eq_ignore_ascii_case("debug") => LevelFilter::Debug,
        s if s.eq_ignore_ascii_case("trace") => LevelFilter::Trace,
        _ => return None,
    })
}

/// The level for `target`: the longest matching `target=level` entry, else the
/// bare default entry, else Info.
fn level_for(target: &str) -> LevelFilter {
    let mut default = LevelFilter::Info;
    let mut best: Option<(usize, LevelFilter)> = None;
    for entry in FILTER.split(',') {
        match entry.split_once('=') {
            None => {
                if let Some(level) = parse_level(entry) {
                    default = level;
                }
            }
            Some((prefix, level)) => {
                let prefix = prefix.trim();
                if let Some(level) = parse_level(level)
                    && target.starts_with(prefix)
                    && best.is_none_or(|(len, _)| prefix.len() > len)
                {
                    best = Some((prefix.len(), level));
                }
            }
        }
    }
    best.map_or(default, |(_, level)| level)
}

fn max_level() -> LevelFilter {
    FILTER
        .split(',')
        .filter_map(|entry| parse_level(entry.rsplit('=').next().unwrap_or(entry)))
        .max()
        .unwrap_or(LevelFilter::Info)
}
