//! Rmedia command line tools library (fftools reproduction).

pub mod cmdline;
pub mod play;
pub mod probe;
pub mod transcode;

pub use cmdline::RmediaOptions;
pub use play::run_play;
pub use probe::run_probe;
pub use transcode::run_transcode;

/// Main entrypoint for rmedia / ffmpeg CLI.
pub fn main_transcode() {
    let args: Vec<String> = std::env::args().collect();
    let opts = RmediaOptions::parse_from_args(&args);
    if let Err(e) = run_transcode(&opts) {
        eprintln!("[rmedia error] {}", e);
        std::process::exit(1);
    }
}

/// Main entrypoint for rmedia-probe / ffprobe CLI.
pub fn main_probe() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(e) = run_probe(&args) {
        eprintln!("[rmedia-probe error] {}", e);
        std::process::exit(1);
    }
}

/// Main entrypoint for rmedia-play / ffplay CLI.
pub fn main_play() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(e) = run_play(&args) {
        eprintln!("[rmedia-play error] {}", e);
        std::process::exit(1);
    }
}
