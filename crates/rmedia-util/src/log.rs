//! Logging subsystem (av_log reproduction).

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;

/// Log levels matching FFmpeg av_log levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum LogLevel {
    Quiet = -8,
    Panic = 0,
    Fatal = 8,
    Error = 16,
    Warning = 24,
    Info = 32,
    Verbose = 40,
    Debug = 48,
    Trace = 56,
}

static CURRENT_LOG_LEVEL: AtomicI32 = AtomicI32::new(LogLevel::Info as i32);

type LogCallback = Box<dyn Fn(LogLevel, &str) + Send + Sync + 'static>;
static LOG_CALLBACK: Mutex<Option<LogCallback>> = Mutex::new(None);

/// Set the global log level.
pub fn set_log_level(level: LogLevel) {
    CURRENT_LOG_LEVEL.store(level as i32, Ordering::SeqCst);
}

/// Get the current global log level.
pub fn get_log_level() -> LogLevel {
    let val = CURRENT_LOG_LEVEL.load(Ordering::SeqCst);
    match val {
        ..=0 => LogLevel::Panic,
        1..=8 => LogLevel::Fatal,
        9..=16 => LogLevel::Error,
        17..=24 => LogLevel::Warning,
        25..=32 => LogLevel::Info,
        33..=40 => LogLevel::Verbose,
        41..=48 => LogLevel::Debug,
        _ => LogLevel::Trace,
    }
}

/// Set a custom logging callback.
pub fn set_log_callback<F>(cb: F)
where
    F: Fn(LogLevel, &str) + Send + Sync + 'static,
{
    let mut guard = LOG_CALLBACK.lock().unwrap();
    *guard = Some(Box::new(cb));
}

/// Log a message at the given log level.
pub fn log(level: LogLevel, msg: &str) {
    let current = get_log_level();
    if level <= current {
        let guard = LOG_CALLBACK.lock().unwrap();
        if let Some(ref cb) = *guard {
            cb(level, msg);
        } else {
            match level {
                LogLevel::Quiet => {}
                LogLevel::Panic | LogLevel::Fatal | LogLevel::Error => {
                    eprintln!("[rmedia error] {}", msg);
                }
                LogLevel::Warning => {
                    eprintln!("[rmedia warning] {}", msg);
                }
                LogLevel::Info => {
                    println!("[rmedia info] {}", msg);
                }
                LogLevel::Verbose | LogLevel::Debug | LogLevel::Trace => {
                    println!("[rmedia debug] {}", msg);
                }
            }
        }
    }
}

#[macro_export]
macro_rules! av_log {
    ($lvl:expr, $($arg:tt)*) => {
        $crate::log::log($lvl, &format!($($arg)*))
    };
}
