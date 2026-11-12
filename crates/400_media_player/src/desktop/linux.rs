//! MPRIS and the single window, over the session bus (D-Bus), with `zbus`.
//!
//! Three objects are published:
//!
//! * `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` at `/org/mpris/MediaPlayer2`: what
//!   media keys, `playerctl` and the media widgets of GNOME and KDE talk to;
//! * `400_media_player.Control` at `/400_media_player/Control`: how a second launch hands its files over.
//!
//! zbus serves the calls on a thread of its own. They only ever post a [`Command`] and wake the
//! interface up; properties are answered from a copy of the last [`State`]. Signals are sent by a
//! worker thread, so neither the interface nor a slow bus can hold the other up.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use media_player_400_core::media_kind::{is_url, path_to_file_url};
use media_player_400_core::playlist::RepeatMode;
use media_player_400_engine::RgbaImage;
use parking_lot::Mutex;
use zbus::blocking::Connection;
use zbus::blocking::connection::Builder;
use zbus::zvariant::{ObjectPath, OwnedValue, Str, Value};

use super::{Command, HandOver, MIME_TYPES, State, Status, Track, URI_SCHEMES, Waker};
use crate::cli::Cli;

/// The name MPRIS players own. A second window (`--new-instance`) gets `….instance<pid>`.
const BUS_NAME: &str = "org.mpris.MediaPlayer2.media_player_400";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const CONTROL_PATH: &str = "/media_player_400/Control";
const CONTROL_INTERFACE: &str = "media_player_400.Control";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

/// How long connecting to the bus, or asking another window to take a launch, may take.
const BUS_TIMEOUT: Duration = Duration::from_secs(3);
/// While playing, the clock is published at least this often, so that the position clients work out
/// from the last one stays true (after a stall, say).
const RESYNC: Duration = Duration::from_secs(1);
/// More arguments than this in one launch is not a launch.
const MAX_ARGS: usize = 100_000;
/// Cover art is shown in small panels: bigger pictures are scaled down before they are written.
const COVER_MAX_SIDE: u32 = 512;

#[derive(Clone, Debug)]
enum Bus {
    Session,
    /// A bus at an explicit address (the tests start a private one).
    #[cfg(test)]
    Address(String),
}

fn builder(bus: &Bus) -> zbus::Result<Builder<'static>> {
    match bus {
        Bus::Session => Builder::session(),
        #[cfg(test)]
        Bus::Address(address) => Builder::address(address.as_str()),
    }
}

/// Runs `f` on a thread of its own and waits for it at most `timeout`: talking to a stuck bus must
/// never hold the program up. What `f` returns after the deadline is dropped.
fn within<T: Send + 'static>(timeout: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("400_media_player-dbus".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .ok()?;
    rx.recv_timeout(timeout).ok()
}

fn track_path(id: u64) -> String {
    if id == 0 { NO_TRACK.to_owned() } else { format!("/media_player_400/track/{id}") }
}

fn loop_status(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "None",
        RepeatMode::One => "Track",
        RepeatMode::All => "Playlist",
    }
}

fn parse_loop_status(s: &str) -> Option<RepeatMode> {
    match s {
        "None" => Some(RepeatMode::Off),
        "Track" => Some(RepeatMode::One),
        "Playlist" => Some(RepeatMode::All),
        _ => None,
    }
}

// =================================================================================================
// Shared state
// =================================================================================================

/// The last published state and when it was published.
struct Live {
    state: State,
    at: Instant,
    /// `file://` URL of the cover art on disk.
    art_url: Option<String>,
}

/// What the bus thread and the interface share.
struct Shared {
    live: Mutex<Live>,
    commands: Mutex<Sender<Command>>,
    waker: Mutex<Option<Waker>>,
}

impl Shared {
    fn new(commands: Sender<Command>) -> Arc<Self> {
        Arc::new(Self { live: Mutex::new(Live { state: State::default(), at: Instant::now(), art_url: None }), commands: Mutex::new(commands), waker: Mutex::new(None) })
    }

    /// Queues a command and wakes the interface up.
    fn send(&self, command: Command) {
        let _ = self.commands.lock().send(command);
        let waker = self.waker.lock().clone();
        if let Some(wake) = waker {
            wake();
        }
    }

    fn read<T>(&self, f: impl FnOnce(&State) -> T) -> T {
        f(&self.live.lock().state)
    }

    /// Microseconds, as the bus wants it.
    fn position_us(&self) -> i64 {
        let live = self.live.lock();
        (live.state.position_after(live.at.elapsed()) * 1e6) as i64
    }

    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let live = self.live.lock();
        metadata(&live.state.track, live.art_url.as_deref())
    }
}

fn string(s: impl Into<String>) -> OwnedValue {
    OwnedValue::from(Str::from(s.into()))
}

fn strings(items: &[String]) -> OwnedValue {
    OwnedValue::try_from(Value::from(items.to_vec())).expect("a list of strings holds no file descriptor")
}

/// The `Metadata` property.
fn metadata(track: &Track, art_url: Option<&str>) -> HashMap<String, OwnedValue> {
    let mut m = HashMap::new();
    let id = ObjectPath::try_from(track_path(track.id)).expect("a valid object path");
    m.insert("mpris:trackid".to_owned(), OwnedValue::from(id));
    if track.id == 0 {
        return m;
    }
    if let Some(length) = track.length {
        m.insert("mpris:length".to_owned(), OwnedValue::from((length * 1e6) as i64));
    }
    if let Some(url) = art_url {
        m.insert("mpris:artUrl".to_owned(), string(url));
    }
    m.insert("xesam:title".to_owned(), string(track.title.as_str()));
    if !track.artists.is_empty() {
        m.insert("xesam:artist".to_owned(), strings(&track.artists));
    }
    if !track.album.is_empty() {
        m.insert("xesam:album".to_owned(), string(track.album.as_str()));
    }
    if let Some(url) = &track.url {
        m.insert("xesam:url".to_owned(), string(url.as_str()));
    }
    m
}

// =================================================================================================
// The objects on the bus
// =================================================================================================

struct Root(Arc<Shared>);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.0.send(Command::Raise);
    }

    fn quit(&self) {
        self.0.send(Command::Quit);
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn fullscreen(&self) -> bool {
        self.0.read(|s| s.fullscreen)
    }

    #[zbus(property)]
    fn set_fullscreen(&mut self, on: bool) {
        self.0.live.lock().state.fullscreen = on;
        self.0.send(Command::SetFullscreen(on));
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_set_fullscreen(&self) -> bool {
        true
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn identity(&self) -> String {
        "400_Player".to_owned()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn desktop_entry(&self) -> String {
        crate::APP_ID.to_owned()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn supported_uri_schemes(&self) -> Vec<String> {
        URI_SCHEMES.iter().map(|s| (*s).to_owned()).collect()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn supported_mime_types(&self) -> Vec<String> {
        MIME_TYPES.iter().map(|s| (*s).to_owned()).collect()
    }
}

struct PlayerIface(Arc<Shared>);

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerIface {
    fn next(&self) {
        self.0.send(Command::Next);
    }

    fn previous(&self) {
        self.0.send(Command::Previous);
    }

    fn pause(&self) {
        self.0.send(Command::Pause);
    }

    fn play_pause(&self) {
        self.0.send(Command::PlayPause);
    }

    fn stop(&self) {
        self.0.send(Command::Stop);
    }

    fn play(&self) {
        self.0.send(Command::Play);
    }

    /// Relative jump, in microseconds.
    fn seek(&self, offset: i64) {
        self.0.send(Command::SeekBy(offset as f64 / 1e6));
    }

    /// Absolute position (microseconds) in the track whose object path is given; a stale track id
    /// means the client is talking about something that is no longer playing.
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        let current = self.0.read(|s| s.track.id);
        if current != 0 && position >= 0 && track_id.as_str() == track_path(current) {
            self.0.send(Command::SeekTo { track: current, secs: position as f64 / 1e6 });
        }
    }

    fn open_uri(&self, uri: String) -> zbus::fdo::Result<()> {
        let scheme = uri.split_once(':').map(|(s, _)| s.to_ascii_lowercase()).unwrap_or_default();
        if !is_url(&uri) && scheme != "file" || !URI_SCHEMES.contains(&scheme.as_str()) {
            return Err(zbus::fdo::Error::InvalidArgs(format!("unsupported URI: {uri}")));
        }
        self.0.send(Command::OpenUri(uri));
        Ok(())
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.0.read(|s| s.status.mpris().to_owned())
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        self.0.read(|s| loop_status(s.repeat).to_owned())
    }

    #[zbus(property)]
    fn set_loop_status(&mut self, value: String) -> zbus::fdo::Result<()> {
        let mode = parse_loop_status(&value).ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("unknown loop status: {value}")))?;
        self.0.live.lock().state.repeat = mode;
        self.0.send(Command::SetRepeat(mode));
        Ok(())
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        self.0.read(|s| s.rate)
    }

    #[zbus(property)]
    fn set_rate(&mut self, rate: f64) {
        // A rate of zero or less is not a speed: it is not accepted.
        if rate.is_finite() && rate > 0.0 {
            let rate = rate.clamp(0.25, 4.0);
            self.0.live.lock().state.rate = rate;
            self.0.send(Command::SetRate(rate));
        }
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.0.read(|s| s.shuffle)
    }

    #[zbus(property)]
    fn set_shuffle(&mut self, on: bool) {
        self.0.live.lock().state.shuffle = on;
        self.0.send(Command::SetShuffle(on));
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.0.metadata()
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.0.read(|s| s.volume)
    }

    #[zbus(property)]
    fn set_volume(&mut self, volume: f64) {
        if volume.is_finite() {
            let volume = volume.clamp(0.0, 2.0);
            self.0.live.lock().state.volume = volume;
            self.0.send(Command::SetVolume(volume));
        }
    }

    /// Clients ask for it when they need it; it is never announced (the spec says so).
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.0.position_us()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn minimum_rate(&self) -> f64 {
        0.25
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn maximum_rate(&self) -> f64 {
        4.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.0.read(|s| s.can_next)
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.0.read(|s| s.can_previous)
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.0.read(|s| s.can_play)
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.0.read(|s| s.can_pause)
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.0.read(|s| s.can_seek)
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }
}

struct ControlIface(Arc<Shared>);

#[zbus::interface(name = "media_player_400.Control")]
impl ControlIface {
    /// Another launch of the player: the arguments of its command line (inputs made absolute).
    fn open(&self, args: Vec<String>) -> zbus::fdo::Result<()> {
        if args.len() > MAX_ARGS {
            return Err(zbus::fdo::Error::InvalidArgs("too many arguments".into()));
        }
        let cli = Cli::from_args(&args).map_err(zbus::fdo::Error::InvalidArgs)?;
        self.0.send(Command::Open(Box::new(cli)));
        Ok(())
    }
}

// =================================================================================================
// Cover art
// =================================================================================================

/// The cover art of the current track, as a PNG file the desktop can read (MPRIS only knows URLs).
struct Art {
    dir: PathBuf,
    current: Option<((u64, usize), PathBuf)>,
}

impl Art {
    fn new() -> Self {
        // The runtime directory is private to the user and emptied at logout.
        Self { dir: dirs::runtime_dir().unwrap_or_else(std::env::temp_dir), current: None }
    }

    fn url_for(&mut self, track: &Track) -> Option<String> {
        let Some(cover) = &track.cover else {
            self.forget();
            return None;
        };
        let key = (track.id, Arc::as_ptr(cover) as usize);
        if let Some((k, path)) = &self.current
            && *k == key
        {
            return Some(path_to_file_url(path));
        }
        self.forget();
        let path = self.dir.join(format!("400_media_player-cover-{}-{}.png", std::process::id(), track.id));
        match write_png(cover, &path) {
            Ok(()) => {
                let url = path_to_file_url(&path);
                self.current = Some((key, path));
                Some(url)
            }
            Err(e) => {
                tracing::debug!("cannot write the cover art for the desktop: {e}");
                None
            }
        }
    }

    fn forget(&mut self) {
        if let Some((_, path)) = self.current.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for Art {
    fn drop(&mut self) {
        self.forget();
    }
}

fn write_png(cover: &RgbaImage, path: &Path) -> Result<(), String> {
    let (w, h) = (cover.width, cover.height);
    if w == 0 || h == 0 || cover.data.len() != w as usize * h as usize * 4 {
        return Err("the picture has no pixels".into());
    }
    let longest = w.max(h);
    if longest <= COVER_MAX_SIDE {
        return image::save_buffer_with_format(path, &cover.data, w, h, image::ColorType::Rgba8, image::ImageFormat::Png).map_err(|e| e.to_string());
    }
    let scale = f64::from(COVER_MAX_SIDE) / f64::from(longest);
    let (nw, nh) = (((f64::from(w) * scale).round() as u32).max(1), ((f64::from(h) * scale).round() as u32).max(1));
    let view = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(w, h, &cover.data[..]).ok_or("bad picture size")?;
    image::imageops::resize(&view, nw, nh, image::imageops::FilterType::Triangle).save_with_format(path, image::ImageFormat::Png).map_err(|e| e.to_string())
}

// =================================================================================================
// The service
// =================================================================================================

enum Msg {
    State { state: State, seeked: bool },
    Quit,
}

/// The player on the bus. Dropping it leaves the bus (and frees the name for the next launch).
pub struct Service {
    shared: Arc<Shared>,
    commands: Receiver<Command>,
    to_worker: Sender<Msg>,
    worker: Option<JoinHandle<()>>,
    last: Option<(State, Instant)>,
    name: String,
}

impl Service {
    /// Joins the session bus. `None` when there is none (no session, a container…): the player works
    /// the same, it is just invisible to the desktop.
    pub fn start() -> Option<Self> {
        Self::start_on(&Bus::Session)
    }

    fn start_on(bus: &Bus) -> Option<Self> {
        let (tx, commands) = channel();
        let shared = Shared::new(tx);
        let (for_thread, bus) = (shared.clone(), bus.clone());
        let connected = within(BUS_TIMEOUT, move || connect(&bus, &for_thread));
        let (connection, name) = match connected {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                tracing::info!("no desktop integration (D-Bus): {e}");
                return None;
            }
            None => {
                tracing::info!("no desktop integration (D-Bus): the bus does not answer");
                return None;
            }
        };
        tracing::info!("desktop integration: {name}");
        let (to_worker, rx) = channel();
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new().name("400_media_player-mpris".into()).spawn(move || worker(&connection, &worker_shared, &rx)).ok()?;
        Some(Self { shared, commands, to_worker, worker: Some(worker), last: None, name })
    }

    /// The window can be woken up when a command arrives.
    pub fn set_waker(&mut self, waker: Waker) {
        *self.shared.waker.lock() = Some(waker);
    }

    /// What the system asked for since the last call. Never blocks.
    pub fn poll(&mut self) -> Vec<Command> {
        self.commands.try_iter().collect()
    }

    /// Tells the system how things are. Cheap to call every frame: nothing is sent unless something
    /// changed (or a second went by while playing).
    pub fn publish(&mut self, state: State) {
        let now = Instant::now();
        let (due, seeked) = match &self.last {
            None => (true, false),
            Some((last, at)) => {
                let elapsed = now.duration_since(*at);
                let seeked = last.jumped_to(&state, elapsed);
                (seeked || !last.same_properties(&state) || (state.status == Status::Playing && elapsed >= RESYNC), seeked)
            }
        };
        if !due {
            return;
        }
        self.last = Some((state.clone(), now));
        let _ = self.to_worker.send(Msg::State { state, seeked });
    }

    #[cfg(test)]
    fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        tracing::debug!("desktop integration: leaving {}", self.name);
        let _ = self.to_worker.send(Msg::Quit);
        // Let the bus see the name go before the process ends, but never wait on a stuck bus.
        if let Some(worker) = self.worker.take() {
            let deadline = Instant::now() + Duration::from_millis(300);
            while !worker.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

/// Connects, publishes the objects and claims the name (or a numbered one if another window has it).
fn connect(bus: &Bus, shared: &Arc<Shared>) -> zbus::Result<(Connection, String)> {
    let build = |name: &str| -> zbus::Result<Connection> {
        builder(bus)?
            .serve_at(MPRIS_PATH, Root(shared.clone()))?
            .serve_at(MPRIS_PATH, PlayerIface(shared.clone()))?
            .serve_at(CONTROL_PATH, ControlIface(shared.clone()))?
            .name(name.to_owned())?
            // zbus lets a newcomer take a name over by default: a second window must never steal
            // the first one's.
            .allow_name_replacements(false)
            .replace_existing_names(false)
            .method_timeout(BUS_TIMEOUT)
            .build()
    };
    match build(BUS_NAME) {
        Err(zbus::Error::NameTaken) => {
            let name = format!("{BUS_NAME}.instance{}", std::process::id());
            build(&name).map(|c| (c, name))
        }
        other => other.map(|c| (c, BUS_NAME.to_owned())),
    }
}

/// The signal thread: applies each published state and announces what changed.
fn worker(conn: &Connection, shared: &Shared, rx: &Receiver<Msg>) {
    let mut art = Art::new();
    'messages: while let Ok(msg) = rx.recv() {
        let (mut state, mut seeked) = match msg {
            Msg::Quit => break,
            Msg::State { state, seeked } => (state, seeked),
        };
        // Only the newest state matters: a burst (dragging the seek bar) is announced once.
        while let Ok(next) = rx.try_recv() {
            match next {
                Msg::Quit => break 'messages,
                Msg::State { state: s, seeked: k } => {
                    state = s;
                    seeked |= k;
                }
            }
        }
        apply(conn, shared, &mut art, state, seeked);
    }
}

fn apply(conn: &Connection, shared: &Shared, art: &mut Art, state: State, seeked: bool) {
    // The cover goes to a file first: encoding takes a moment, and no lock is held meanwhile.
    let art_url = art.url_for(&state.track);
    let (old, old_art) = {
        let mut live = shared.live.lock();
        let old = std::mem::replace(&mut live.state, state.clone());
        let old_art = std::mem::replace(&mut live.art_url, art_url.clone());
        live.at = Instant::now();
        (old, old_art)
    };
    let player = changed_player_properties(&old, &old_art, &state, &art_url);
    announce(conn, PLAYER_INTERFACE, player);
    if old.fullscreen != state.fullscreen {
        announce(conn, ROOT_INTERFACE, HashMap::from([("Fullscreen", OwnedValue::from(state.fullscreen))]));
    }
    if seeked {
        let position = (state.position * 1e6) as i64;
        if let Err(e) = conn.emit_signal(None::<&str>, MPRIS_PATH, PLAYER_INTERFACE, "Seeked", &(position,)) {
            tracing::debug!("cannot announce a seek: {e}");
        }
    }
}

/// The `Player` properties that differ between two states (the clock is not one of them).
fn changed_player_properties(old: &State, old_art: &Option<String>, new: &State, new_art: &Option<String>) -> HashMap<&'static str, OwnedValue> {
    let mut changed = HashMap::new();
    if old.status != new.status {
        changed.insert("PlaybackStatus", string(new.status.mpris()));
    }
    if old.repeat != new.repeat {
        changed.insert("LoopStatus", string(loop_status(new.repeat)));
    }
    if old.rate != new.rate {
        changed.insert("Rate", OwnedValue::from(new.rate));
    }
    if old.shuffle != new.shuffle {
        changed.insert("Shuffle", OwnedValue::from(new.shuffle));
    }
    if old.volume != new.volume {
        changed.insert("Volume", OwnedValue::from(new.volume));
    }
    if !Arc::ptr_eq(&old.track, &new.track) || old_art != new_art {
        let map = metadata(&new.track, new_art.as_deref());
        changed.insert("Metadata", OwnedValue::try_from(Value::from(map)).expect("metadata holds no file descriptor"));
    }
    for (name, was, is) in [
        ("CanGoNext", old.can_next, new.can_next),
        ("CanGoPrevious", old.can_previous, new.can_previous),
        ("CanPlay", old.can_play, new.can_play),
        ("CanPause", old.can_pause, new.can_pause),
        ("CanSeek", old.can_seek, new.can_seek),
    ] {
        if was != is {
            changed.insert(name, OwnedValue::from(is));
        }
    }
    changed
}

/// `org.freedesktop.DBus.Properties.PropertiesChanged`.
fn announce(conn: &Connection, interface: &str, changed: HashMap<&str, OwnedValue>) {
    if changed.is_empty() {
        return;
    }
    let body = (interface, changed, Vec::<&str>::new());
    if let Err(e) = conn.emit_signal(None::<&str>, MPRIS_PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged", &body) {
        tracing::debug!("cannot announce a change: {e}");
    }
}

// =================================================================================================
// Handing a launch over
// =================================================================================================

/// Offers this launch to the window that already runs, if there is one.
pub fn hand_over(cli: &Cli) -> HandOver {
    hand_over_on(&Bus::Session, cli)
}

fn hand_over_on(bus: &Bus, cli: &Cli) -> HandOver {
    let mut request = cli.clone();
    request.inputs = super::absolutize(&cli.inputs);
    let args = request.to_args();
    let bus = bus.clone();
    within(BUS_TIMEOUT + Duration::from_secs(1), move || ask(&bus, &args)).unwrap_or_else(|| HandOver::Failed("the bus did not answer in time".into()))
}

fn ask(bus: &Bus, args: &[String]) -> HandOver {
    let connection = match builder(bus).and_then(|b| b.method_timeout(BUS_TIMEOUT).build()) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("no session bus: {e}");
            return HandOver::NoRunningWindow;
        }
    };
    let owned = connection
        .call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "NameHasOwner", &(BUS_NAME,))
        .and_then(|reply| reply.body().deserialize::<bool>());
    match owned {
        Ok(true) => {}
        Ok(false) => return HandOver::NoRunningWindow,
        Err(e) => {
            tracing::debug!("the bus cannot say who owns {BUS_NAME}: {e}");
            return HandOver::NoRunningWindow;
        }
    }
    match connection.call_method(Some(BUS_NAME), CONTROL_PATH, Some(CONTROL_INTERFACE), "Open", &(args,)) {
        Ok(_) => HandOver::Delivered,
        Err(e) => HandOver::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command as Process, Stdio};

    use super::*;
    use crate::desktop::Track;

    /// A private session bus (`dbus-daemon`), so that the tests never touch the real one. Tests are
    /// skipped (with a note) on a machine without `dbus-daemon`.
    struct PrivateBus {
        child: Child,
        address: String,
    }

    impl PrivateBus {
        fn start() -> Option<Self> {
            let mut child = Process::new("dbus-daemon").args(["--session", "--nofork", "--print-address=1"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
            let mut line = String::new();
            BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
            let address = line.trim().to_owned();
            if address.is_empty() {
                let _ = child.kill();
                return None;
            }
            Some(Self { child, address })
        }

        fn bus(&self) -> Bus {
            Bus::Address(self.address.clone())
        }

        fn client(&self) -> Connection {
            builder(&self.bus()).unwrap().method_timeout(Duration::from_secs(5)).build().expect("a client on the private bus")
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    macro_rules! private_bus {
        () => {
            match PrivateBus::start() {
                Some(bus) => bus,
                None => {
                    eprintln!("skipped: dbus-daemon is not available");
                    return;
                }
            }
        };
    }

    fn get(client: &Connection, name: &str, interface: &str, property: &str) -> OwnedValue {
        let reply =
            client.call_method(Some(name), MPRIS_PATH, Some("org.freedesktop.DBus.Properties"), "Get", &(interface, property)).unwrap_or_else(|e| panic!("Get {property}: {e}"));
        reply.body().deserialize::<OwnedValue>().unwrap()
    }

    fn set(client: &Connection, name: &str, property: &str, value: Value<'_>) -> zbus::Result<()> {
        client.call_method(Some(name), MPRIS_PATH, Some("org.freedesktop.DBus.Properties"), "Set", &(PLAYER_INTERFACE, property, value)).map(|_| ())
    }

    fn call(client: &Connection, name: &str, member: &str) {
        client.call_method(Some(name), MPRIS_PATH, Some(PLAYER_INTERFACE), member, &()).unwrap_or_else(|e| panic!("{member}: {e}"));
    }

    fn wait_for<T>(mut probe: impl FnMut() -> Option<T>) -> T {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(v) = probe() {
                return v;
            }
            assert!(Instant::now() < end, "timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn song(id: u64) -> Arc<Track> {
        Arc::new(Track {
            id,
            title: "Nocturne".into(),
            artists: vec!["Chopin".into()],
            album: "Op. 9".into(),
            url: Some("file:///music/n.flac".into()),
            length: Some(240.0),
            cover: None,
        })
    }

    #[test]
    fn the_player_describes_itself_and_follows_what_is_published() {
        let bus = private_bus!();
        let mut service = Service::start_on(&bus.bus()).expect("the service starts");
        assert_eq!(service.name(), BUS_NAME);
        let client = bus.client();

        // Before anything plays.
        assert_eq!(String::try_from(get(&client, BUS_NAME, ROOT_INTERFACE, "Identity")).unwrap(), "400_Player");
        assert_eq!(String::try_from(get(&client, BUS_NAME, ROOT_INTERFACE, "DesktopEntry")).unwrap(), crate::APP_ID);
        assert_eq!(String::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "PlaybackStatus")).unwrap(), "Stopped");
        assert!(bool::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "CanControl")).unwrap());
        assert!(!bool::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "CanPlay")).unwrap());
        let schemes = Vec::<String>::try_from(get(&client, BUS_NAME, ROOT_INTERFACE, "SupportedUriSchemes")).unwrap();
        assert!(schemes.iter().any(|s| s == "https"));
        let meta = HashMap::<String, OwnedValue>::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "Metadata")).unwrap();
        assert_eq!(meta.len(), 1, "an empty player only has the 'no track' id: {meta:?}");

        // A song starts.
        service.publish(State {
            status: Status::Playing,
            position: 30.0,
            volume: 0.8,
            can_play: true,
            can_pause: true,
            can_seek: true,
            can_next: true,
            repeat: RepeatMode::All,
            track: song(7),
            ..Default::default()
        });
        wait_for(|| (String::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "PlaybackStatus")).ok()? == "Playing").then_some(()));
        assert_eq!(String::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "LoopStatus")).unwrap(), "Playlist");
        assert!((f64::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "Volume")).unwrap() - 0.8).abs() < 1e-9);
        assert!(bool::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "CanGoNext")).unwrap());
        let meta = HashMap::<String, OwnedValue>::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "Metadata")).unwrap();
        assert_eq!(String::try_from(meta["xesam:title"].try_clone().unwrap()).unwrap(), "Nocturne");
        assert_eq!(Vec::<String>::try_from(meta["xesam:artist"].try_clone().unwrap()).unwrap(), vec!["Chopin".to_owned()]);
        assert_eq!(i64::try_from(meta["mpris:length"].try_clone().unwrap()).unwrap(), 240_000_000);
        assert_eq!(String::try_from(meta["xesam:url"].try_clone().unwrap()).unwrap(), "file:///music/n.flac");
        let id = ObjectPath::try_from(meta["mpris:trackid"].try_clone().unwrap()).unwrap();
        assert_eq!(id.as_str(), "/media_player_400/track/7");

        // The position keeps moving between two publications: clients ask for it when they need it.
        let first = i64::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "Position")).unwrap();
        assert!((30_000_000..32_000_000).contains(&first), "{first}");
        std::thread::sleep(Duration::from_millis(250));
        let second = i64::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "Position")).unwrap();
        assert!(second >= first + 200_000, "the clock advances by itself: {first} → {second}");

        // Paused: it stops.
        service.publish(State { status: Status::Paused, position: 41.0, track: song(7), ..Default::default() });
        wait_for(|| (String::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "PlaybackStatus")).ok()? == "Paused").then_some(()));
        let a = i64::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "Position")).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(a, i64::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "Position")).unwrap());
    }

    #[test]
    fn commands_from_the_desktop_reach_the_player() {
        let bus = private_bus!();
        let mut service = Service::start_on(&bus.bus()).expect("the service starts");
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = woken.clone();
        service.set_waker(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        service.publish(State { status: Status::Playing, track: song(3), can_play: true, ..Default::default() });
        let client = bus.client();
        wait_for(|| (String::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "PlaybackStatus")).ok()? == "Playing").then_some(()));

        for member in ["PlayPause", "Play", "Pause", "Next", "Previous", "Stop"] {
            call(&client, BUS_NAME, member);
        }
        client.call_method(Some(BUS_NAME), MPRIS_PATH, Some(PLAYER_INTERFACE), "Seek", &(-5_000_000_i64,)).unwrap();
        let track = ObjectPath::try_from("/media_player_400/track/3").unwrap();
        client.call_method(Some(BUS_NAME), MPRIS_PATH, Some(PLAYER_INTERFACE), "SetPosition", &(&track, 42_000_000_i64)).unwrap();
        // A stale track id is ignored.
        let stale = ObjectPath::try_from("/media_player_400/track/2").unwrap();
        client.call_method(Some(BUS_NAME), MPRIS_PATH, Some(PLAYER_INTERFACE), "SetPosition", &(&stale, 1_000_000_i64)).unwrap();
        set(&client, BUS_NAME, "Volume", Value::from(0.5_f64)).unwrap();
        set(&client, BUS_NAME, "LoopStatus", Value::from("Track")).unwrap();
        set(&client, BUS_NAME, "Shuffle", Value::from(true)).unwrap();
        set(&client, BUS_NAME, "Rate", Value::from(1.5_f64)).unwrap();
        assert!(set(&client, BUS_NAME, "LoopStatus", Value::from("Sideways")).is_err(), "unknown loop status");
        client.call_method(Some(BUS_NAME), MPRIS_PATH, Some(PLAYER_INTERFACE), "OpenUri", &("https://example.org/live.m3u8",)).unwrap();
        assert!(client.call_method(Some(BUS_NAME), MPRIS_PATH, Some(PLAYER_INTERFACE), "OpenUri", &("javascript:alert(1)",)).is_err());
        client.call_method(Some(BUS_NAME), MPRIS_PATH, Some(ROOT_INTERFACE), "Raise", &()).unwrap();
        client.call_method(Some(BUS_NAME), MPRIS_PATH, Some(ROOT_INTERFACE), "Quit", &()).unwrap();

        let mut all = Vec::new();
        wait_for(|| {
            all.extend(service.poll());
            (all.len() >= 15).then_some(())
        });
        std::thread::sleep(Duration::from_millis(50));
        all.extend(service.poll());
        let text: Vec<String> = all.iter().map(|c| format!("{c:?}")).collect();
        assert_eq!(
            text,
            [
                "PlayPause",
                "Play",
                "Pause",
                "Next",
                "Previous",
                "Stop",
                "SeekBy(-5.0)",
                "SeekTo { track: 3, secs: 42.0 }",
                "SetVolume(0.5)",
                "SetRepeat(One)",
                "SetShuffle(true)",
                "SetRate(1.5)",
                "OpenUri(\"https://example.org/live.m3u8\")",
                "Raise",
                "Quit",
            ],
            "commands arrive in order, and the refused ones do not arrive"
        );
        assert!(woken.load(std::sync::atomic::Ordering::SeqCst) >= 15, "every command wakes the window");
    }

    #[test]
    fn changes_and_seeks_are_announced() {
        let bus = private_bus!();
        let mut service = Service::start_on(&bus.bus()).expect("the service starts");
        let client = bus.client();
        let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).path(MPRIS_PATH).unwrap().build();
        let mut stream = zbus::blocking::MessageIterator::for_match_rule(rule, &client, None).expect("a match rule");

        let track = song(1);
        service.publish(State { status: Status::Playing, position: 10.0, track: track.clone(), can_play: true, ..Default::default() });
        // PropertiesChanged: status, metadata, CanPlay.
        let message = stream.next().expect("a signal").unwrap();
        let header = message.header();
        assert_eq!(header.member().map(|m| m.as_str()), Some("PropertiesChanged"));
        let (interface, changed, _invalidated): (String, HashMap<String, OwnedValue>, Vec<String>) = message.body().deserialize().unwrap();
        assert_eq!(interface, PLAYER_INTERFACE);
        assert_eq!(String::try_from(changed["PlaybackStatus"].try_clone().unwrap()).unwrap(), "Playing");
        assert!(changed.contains_key("Metadata") && changed.contains_key("CanPlay"));
        assert!(!changed.contains_key("Position"), "the clock is never announced");

        // A jump of the clock is a Seeked.
        service.publish(State { status: Status::Playing, position: 120.0, track, can_play: true, ..Default::default() });
        let message = stream.next().expect("a signal").unwrap();
        assert_eq!(message.header().member().map(|m| m.as_str()), Some("Seeked"));
        assert_eq!(message.body().deserialize::<i64>().unwrap(), 120_000_000);
    }

    #[test]
    fn cover_art_is_written_as_a_file_the_desktop_can_read() {
        let bus = private_bus!();
        let mut service = Service::start_on(&bus.bus()).expect("the service starts");
        let client = bus.client();
        let cover = Arc::new(RgbaImage { width: 1200, height: 900, data: vec![200; 1200 * 900 * 4] });
        let track = Arc::new(Track { id: 11, title: "Cover".into(), cover: Some(cover), ..Default::default() });
        service.publish(State { status: Status::Playing, track, can_play: true, ..Default::default() });
        let url = wait_for(|| {
            let meta = HashMap::<String, OwnedValue>::try_from(get(&client, BUS_NAME, PLAYER_INTERFACE, "Metadata")).ok()?;
            String::try_from(meta.get("mpris:artUrl")?.try_clone().ok()?).ok()
        });
        assert!(url.starts_with("file://"), "{url}");
        let path = media_player_400_core::media_kind::file_url_to_path(&url).unwrap();
        let png = image::open(&path).expect("a readable picture");
        assert_eq!((png.width(), png.height()), (COVER_MAX_SIDE, 384), "scaled down, proportions kept");
        // Stopping removes the file.
        service.publish(State::default());
        wait_for(|| (!path.exists()).then_some(()));
    }

    #[test]
    fn a_second_launch_gives_its_files_to_the_running_window() {
        let bus = private_bus!();
        let args: Vec<String> = ["--paused", "--start=1:30", "--", "movie.mkv", "https://example.org/a.m3u8"].iter().map(|s| (*s).to_owned()).collect();
        let launch = Cli::from_args(&args).unwrap();

        assert_eq!(hand_over_on(&bus.bus(), &launch), HandOver::NoRunningWindow, "nobody runs yet");
        let mut service = Service::start_on(&bus.bus()).expect("the service starts");
        assert_eq!(hand_over_on(&bus.bus(), &launch), HandOver::Delivered);

        let got = wait_for(|| {
            std::thread::sleep(Duration::from_millis(10));
            service.poll().into_iter().next()
        });
        let Command::Open(cli) = got else { panic!("expected a launch, got {got:?}") };
        assert!(cli.paused);
        assert_eq!(cli.start.as_deref(), Some("1:30"));
        assert!(Path::new(&cli.inputs[0]).is_absolute() && cli.inputs[0].ends_with("movie.mkv"), "relative paths are made absolute: {:?}", cli.inputs);
        assert_eq!(cli.inputs[1], "https://example.org/a.m3u8");

        // A bare launch only asks for the window to come forward.
        assert_eq!(hand_over_on(&bus.bus(), &Cli::default()), HandOver::Delivered);
        let Command::Open(bare) = wait_for(|| service.poll().into_iter().next()) else { panic!("expected a launch") };
        assert!(bare.inputs.is_empty());

        // Nonsense is refused, not swallowed.
        let client = bus.client();
        let refused = client.call_method(Some(BUS_NAME), CONTROL_PATH, Some(CONTROL_INTERFACE), "Open", &(vec!["--bogus".to_owned()],));
        assert!(refused.is_err());

        // A second window on the same bus gets a numbered name and does not steal the first one.
        let second = Service::start_on(&bus.bus()).expect("the second service starts");
        assert_eq!(second.name(), format!("{BUS_NAME}.instance{}", std::process::id()));
        assert_eq!(hand_over_on(&bus.bus(), &launch), HandOver::Delivered);
        assert!(matches!(wait_for(|| service.poll().into_iter().next()), Command::Open(_)));

        // Closing the window frees the name: the next launch starts a new one.
        drop(service);
        drop(second);
        wait_for(|| (hand_over_on(&bus.bus(), &launch) == HandOver::NoRunningWindow).then_some(()));
    }

    #[test]
    fn without_a_bus_everything_is_quiet() {
        let nowhere = Bus::Address("unix:path=/nonexistent/400_media_player-test-bus".into());
        assert!(Service::start_on(&nowhere).is_none());
        assert_eq!(hand_over_on(&nowhere, &Cli::default()), HandOver::NoRunningWindow);
    }

    #[test]
    fn loop_statuses_round_trip() {
        for mode in [RepeatMode::Off, RepeatMode::One, RepeatMode::All] {
            assert_eq!(parse_loop_status(loop_status(mode)), Some(mode));
        }
        assert_eq!(parse_loop_status("none"), None);
    }
}
