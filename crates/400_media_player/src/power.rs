//! Keeps the screen on (and the computer awake) while something plays, through the operating
//! system's own inhibition mechanism: D-Bus (`org.freedesktop.ScreenSaver`, logind) on Linux,
//! power assertions on macOS and `SetThreadExecutionState` on Windows (all through `keepawake`).
//!
//! Talking to the system can block (a slow or absent D-Bus service), so it happens on a thread of its
//! own: [`Awake::set`] only posts what is needed. A refusal is logged once and retried now and
//! then; it never gets in the way of playback.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long to wait before trying again after the system refused.
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// What the current playback needs the system to stay awake for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// Nothing is playing: the system is free to blank the screen and to sleep.
    Nothing,
    /// Sound only: the computer must not suspend, the screen may go dark.
    Sound,
    /// A picture: the screen stays on as well.
    Picture,
}

impl Need {
    pub fn of(playing: bool, has_video: bool) -> Self {
        match (playing, has_video) {
            (false, _) => Need::Nothing,
            (true, false) => Need::Sound,
            (true, true) => Need::Picture,
        }
    }
}

/// The handle the interface holds: dropping it releases everything.
pub struct Awake {
    need: Need,
    tx: Option<Sender<Need>>,
    worker: Option<JoinHandle<()>>,
}

impl Default for Awake {
    fn default() -> Self {
        Self { need: Need::Nothing, tx: None, worker: None }
    }
}

impl Awake {
    /// Tells the system what playback needs. Cheap when nothing changes, so it can run every frame.
    pub fn set(&mut self, need: Need) {
        if need == self.need {
            return;
        }
        tracing::debug!("system wake-up need: {:?} → {need:?}", self.need);
        self.need = need;
        if self.tx.is_none() {
            if need == Need::Nothing {
                return;
            }
            let (tx, rx) = channel();
            let worker = std::thread::Builder::new().name("400_media_player-awake".into()).spawn(move || run(&rx, acquire, RETRY_AFTER));
            match worker {
                Ok(handle) => {
                    self.tx = Some(tx);
                    self.worker = Some(handle);
                }
                Err(e) => {
                    tracing::warn!("cannot start the wake-lock thread: {e}");
                    return;
                }
            }
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(need);
        }
    }
}

impl Drop for Awake {
    fn drop(&mut self) {
        // Closing the channel makes the worker release what it holds; wait a moment so the release
        // reaches the system before the process ends, but never hang on a stuck service.
        self.tx = None;
        if let Some(worker) = self.worker.take() {
            let deadline = Instant::now() + Duration::from_millis(600);
            while !worker.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

/// A `keepawake` guard whose release can never take the thread down (the library unwraps the D-Bus
/// reply when it lets go).
struct Held(Option<keepawake::KeepAwake>);

impl Drop for Held {
    fn drop(&mut self) {
        if let Some(guard) = self.0.take() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(guard)));
        }
    }
}

fn acquire(screen: bool) -> Result<Held, String> {
    let guard = keepawake::Builder::default()
        .display(screen)
        .idle(!screen)
        .reason("Media playback")
        .app_name("400_Player")
        .app_reverse_domain(crate::APP_ID)
        .create()
        .map_err(|e| e.to_string())?;
    tracing::debug!("keeping the {} awake", if screen { "screen" } else { "computer" });
    Ok(Held(Some(guard)))
}

/// The worker: holds one guard for the screen and one for the computer, as `Need` says. A guard that
/// cannot be had is tried again after `retry`, or as soon as the need changes.
fn run<G>(rx: &Receiver<Need>, mut acquire: impl FnMut(bool) -> Result<G, String>, retry: Duration) {
    let mut want = Need::Nothing;
    let mut screen: Option<G> = None;
    let mut computer: Option<G> = None;
    let mut retry_at: Option<Instant> = None;
    let mut warned = false;
    loop {
        let message = match retry_at {
            Some(t) => rx.recv_timeout(t.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match message {
            Ok(n) => {
                want = n;
                while let Ok(n) = rx.try_recv() {
                    want = n;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let want_screen = want == Need::Picture;
        let want_computer = want != Need::Nothing;
        if !want_screen {
            screen = None;
        }
        if !want_computer {
            computer = None;
        }
        retry_at = None;
        for (is_screen, wanted) in [(true, want_screen), (false, want_computer)] {
            let slot = if is_screen { &mut screen } else { &mut computer };
            if !wanted || slot.is_some() {
                continue;
            }
            match acquire(is_screen) {
                Ok(guard) => *slot = Some(guard),
                Err(e) => {
                    if !warned {
                        warned = true;
                        tracing::info!("cannot keep the system awake: {e}");
                    }
                    retry_at = Some(Instant::now() + retry);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn what_is_needed() {
        assert_eq!(Need::of(false, true), Need::Nothing);
        assert_eq!(Need::of(false, false), Need::Nothing);
        assert_eq!(Need::of(true, false), Need::Sound);
        assert_eq!(Need::of(true, true), Need::Picture);
    }

    type Log = Arc<Mutex<Vec<String>>>;

    /// A guard that records its release.
    struct Fake(Log, &'static str);

    impl Drop for Fake {
        fn drop(&mut self) {
            self.0.lock().unwrap().push(format!("release {}", self.1));
        }
    }

    fn name(screen: bool) -> &'static str {
        if screen { "screen" } else { "computer" }
    }

    fn wait_for(log: &Log, what: &str) -> bool {
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            if log.lock().unwrap().iter().any(|l| l == what) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    fn spawn_fake(fail_computer_times: usize) -> (Sender<Need>, JoinHandle<()>, Log) {
        let log: Log = Arc::default();
        let (tx, rx) = channel();
        let l = log.clone();
        let mut failures_left = fail_computer_times;
        let handle = std::thread::spawn(move || {
            run(
                &rx,
                |screen| {
                    if !screen && failures_left > 0 {
                        failures_left -= 1;
                        l.lock().unwrap().push("refused computer".into());
                        return Err("no service".into());
                    }
                    l.lock().unwrap().push(format!("acquire {}", name(screen)));
                    Ok(Fake(l.clone(), name(screen)))
                },
                Duration::from_millis(40),
            );
        });
        (tx, handle, log)
    }

    #[test]
    fn the_worker_holds_what_is_needed_and_lets_go_of_the_rest() {
        let (tx, handle, log) = spawn_fake(0);
        tx.send(Need::Picture).unwrap();
        assert!(wait_for(&log, "acquire screen") && wait_for(&log, "acquire computer"));
        tx.send(Need::Sound).unwrap();
        assert!(wait_for(&log, "release screen"), "music alone does not need the screen");
        tx.send(Need::Nothing).unwrap();
        assert!(wait_for(&log, "release computer"));
        tx.send(Need::Picture).unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        while log.lock().unwrap().iter().filter(|l| l.starts_with("acquire")).count() < 4 && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Dropping the sender (the window closing) releases everything.
        drop(tx);
        handle.join().unwrap();
        let log = log.lock().unwrap();
        let acquired = log.iter().filter(|l| l.starts_with("acquire")).count();
        let released = log.iter().filter(|l| l.starts_with("release")).count();
        assert_eq!(acquired, 4, "{log:?}");
        assert_eq!(released, 4, "everything taken is given back: {log:?}");
    }

    #[test]
    fn a_refusal_does_not_block_the_other_guard_and_is_retried() {
        let (tx, handle, log) = spawn_fake(2);
        tx.send(Need::Picture).unwrap();
        assert!(wait_for(&log, "acquire screen"), "the screen is not held up by the computer guard");
        assert!(wait_for(&log, "acquire computer"), "the refused guard is retried: {:?}", log.lock().unwrap());
        assert_eq!(log.lock().unwrap().iter().filter(|l| *l == "refused computer").count(), 2);
        drop(tx);
        handle.join().unwrap();
    }

    #[test]
    fn the_handle_never_blocks_the_caller() {
        // Whatever the machine offers (a session bus or none), posting needs returns at once.
        let mut awake = Awake::default();
        let t0 = Instant::now();
        awake.set(Need::Picture);
        awake.set(Need::Nothing);
        awake.set(Need::Sound);
        assert!(t0.elapsed() < Duration::from_millis(250), "set() took {:?}", t0.elapsed());
        drop(awake);
    }
}
