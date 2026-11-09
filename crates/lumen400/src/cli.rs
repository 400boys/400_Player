//! Command line.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use lumen400_core::settings::{Accent, Language, ThemeMode};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ThemeArg {
    Dark,
    Oled,
    Light,
    System,
}

impl From<ThemeArg> for ThemeMode {
    fn from(t: ThemeArg) -> Self {
        match t {
            ThemeArg::Dark => ThemeMode::Dark,
            ThemeArg::Oled => ThemeMode::Oled,
            ThemeArg::Light => ThemeMode::Light,
            ThemeArg::System => ThemeMode::System,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum LangArg {
    Auto,
    Fr,
    En,
}

impl From<LangArg> for Language {
    fn from(l: LangArg) -> Self {
        match l {
            LangArg::Auto => Language::Auto,
            LangArg::Fr => Language::French,
            LangArg::En => Language::English,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AccentArg {
    Violet,
    Sky,
    Emerald,
    Rose,
    Amber,
    Orange,
    Fuchsia,
    Cyan,
}

impl From<AccentArg> for Accent {
    fn from(a: AccentArg) -> Self {
        match a {
            AccentArg::Violet => Accent::Violet,
            AccentArg::Sky => Accent::Sky,
            AccentArg::Emerald => Accent::Emerald,
            AccentArg::Rose => Accent::Rose,
            AccentArg::Amber => Accent::Amber,
            AccentArg::Orange => Accent::Orange,
            AccentArg::Fuchsia => Accent::Fuchsia,
            AccentArg::Cyan => Accent::Cyan,
        }
    }
}

/// 400_Player — a modern, fast media player.
#[derive(Parser, Clone, Debug, Default)]
#[command(name = "400_Player", version, about, long_about = None, after_help = "Keyboard shortcuts: press F1 in the player, or Ctrl+K for the command palette.")]
pub struct Cli {
    /// Files, folders, playlists (m3u, pls, xspf) or network URLs to play.
    #[arg(value_name = "FILE|URL")]
    pub inputs: Vec<String>,

    /// Print what is known about each input (streams, codecs, chapters…) as JSON and exit.
    #[arg(long)]
    pub info: bool,

    /// Start in full screen.
    #[arg(short, long)]
    pub fullscreen: bool,

    /// Open the media paused.
    #[arg(long)]
    pub paused: bool,

    /// Start at this position: seconds, `mm:ss`, `h:mm:ss` or `1h2m3s`.
    #[arg(long, value_name = "TIME")]
    pub start: Option<String>,

    /// Volume in percent (0–200).
    #[arg(long, value_name = "PERCENT")]
    pub volume: Option<f32>,

    /// Start muted.
    #[arg(long)]
    pub mute: bool,

    /// Colour theme (saved in the settings).
    #[arg(long, value_enum)]
    pub theme: Option<ThemeArg>,

    /// Accent colour (saved in the settings).
    #[arg(long, value_enum)]
    pub accent: Option<AccentArg>,

    /// Interface language (saved in the settings).
    #[arg(long, value_enum)]
    pub lang: Option<LangArg>,

    /// Never open the sound card: audio is decoded and timed, but not played.
    #[arg(long)]
    pub virtual_audio: bool,

    /// Do not read or write the settings and history of the user.
    #[arg(long)]
    pub ephemeral: bool,

    /// Add the files to the queue of the running window instead of replacing it.
    #[arg(short = 'a', long)]
    pub enqueue: bool,

    /// Open a new window even if 400_Player is already running.
    #[arg(long)]
    pub new_instance: bool,

    /// Run an automation script (interface tests, demos).
    #[arg(long, value_name = "SCRIPT", hide = true)]
    pub automation: Option<PathBuf>,

    /// Join the desktop (media keys, `playerctl`, single window) even with `--ephemeral`: for tests.
    #[arg(long, hide = true)]
    pub desktop_integration: bool,
}

impl Cli {
    /// `--start` as seconds.
    pub fn start_seconds(&self) -> Option<f64> {
        self.start.as_deref().and_then(lumen400_core::time::parse_timecode)
    }

    /// Should this process join the desktop (media keys, `playerctl`, one window)? Tests and demos
    /// (`--ephemeral`, `--automation`) stay out of the user's session unless they ask for it.
    pub fn joins_desktop(&self) -> bool {
        !self.info && (self.desktop_integration || (!self.ephemeral && self.automation.is_none()))
    }

    /// Should this launch first offer its request to a window that already runs?
    pub fn may_join_running_window(&self) -> bool {
        self.joins_desktop() && !self.new_instance
    }

    /// The arguments (without the program name) that make the same request again. They travel to the
    /// running window, which parses them like any command line; `inputs` should be absolute by then.
    #[allow(dead_code)]
    pub fn to_args(&self) -> Vec<String> {
        fn name<T: ValueEnum>(v: &T) -> String {
            v.to_possible_value().map(|p| p.get_name().to_owned()).unwrap_or_default()
        }
        let mut a = Vec::new();
        for (on, flag) in [(self.fullscreen, "--fullscreen"), (self.paused, "--paused"), (self.mute, "--mute"), (self.enqueue, "--enqueue")] {
            if on {
                a.push(flag.to_owned());
            }
        }
        if let Some(s) = &self.start {
            a.push(format!("--start={s}"));
        }
        if let Some(v) = self.volume {
            a.push(format!("--volume={v}"));
        }
        if let Some(t) = &self.theme {
            a.push(format!("--theme={}", name(t)));
        }
        if let Some(c) = &self.accent {
            a.push(format!("--accent={}", name(c)));
        }
        if let Some(l) = &self.lang {
            a.push(format!("--lang={}", name(l)));
        }
        if !self.inputs.is_empty() {
            // After `--`, so that a file called `-weird.mkv` is still a file.
            a.push("--".to_owned());
            a.extend(self.inputs.iter().cloned());
        }
        a
    }

    /// Parses what [`Cli::to_args`] produced.
    #[allow(dead_code)]
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        Self::try_parse_from(std::iter::once("lumen400").chain(args.iter().map(String::as_str))).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_typical_invocations() {
        let cli = Cli::parse_from(["lumen400", "a.mkv", "b.mp4", "--start", "1:30", "--volume", "80", "--paused", "-f"]);
        assert_eq!(cli.inputs, vec!["a.mkv", "b.mp4"]);
        assert_eq!(cli.start_seconds(), Some(90.0));
        assert_eq!(cli.volume, Some(80.0));
        assert!(cli.paused && cli.fullscreen && !cli.info);
        let cli = Cli::parse_from(["lumen400", "--theme", "oled", "--accent", "rose", "--lang", "fr"]);
        assert!(matches!(cli.theme, Some(ThemeArg::Oled)));
        assert_eq!(Accent::from(cli.accent.unwrap()), Accent::Rose);
        assert_eq!(Language::from(cli.lang.unwrap()), Language::French);
        assert!(Cli::try_parse_from(["lumen400", "--theme", "neon"]).is_err());
    }

    #[test]
    fn a_launch_survives_the_trip_to_the_running_window() {
        let cli = Cli::parse_from([
            "lumen400",
            "--paused",
            "-f",
            "--mute",
            "-a",
            "--start",
            "1:30",
            "--volume",
            "80",
            "--theme",
            "oled",
            "--accent",
            "rose",
            "--lang",
            "fr",
            "/a b/-odd.mkv",
            "https://x.org/s.m3u8",
        ]);
        let back = Cli::from_args(&cli.to_args()).expect("the arguments parse");
        assert_eq!(format!("{cli:?}"), format!("{back:?}"));
        assert_eq!(back.inputs, vec!["/a b/-odd.mkv", "https://x.org/s.m3u8"]);
        // A bare launch is a request to raise the window.
        let bare = Cli::from_args(&Cli::default().to_args()).unwrap();
        assert!(bare.inputs.is_empty() && !bare.paused);
        assert!(Cli::from_args(&["--nonsense".to_owned()]).is_err());
    }

    #[test]
    fn only_plain_launches_join_the_desktop() {
        assert!(Cli::parse_from(["lumen400", "a.mkv"]).may_join_running_window());
        assert!(Cli::parse_from(["lumen400", "--virtual-audio", "a.mkv"]).may_join_running_window());
        for flags in [&["--info"][..], &["--ephemeral"], &["--automation", "x.txt"]] {
            let cli = Cli::parse_from(["lumen400"].into_iter().chain(flags.iter().copied()).chain(["a.mkv"]));
            assert!(!cli.joins_desktop(), "{flags:?}");
        }
        let cli = Cli::parse_from(["lumen400", "--new-instance", "a.mkv"]);
        assert!(cli.joins_desktop() && !cli.may_join_running_window(), "a new window is still announced on the bus");
        let test_run = Cli::parse_from(["lumen400", "--ephemeral", "--automation", "x.txt", "--desktop-integration"]);
        assert!(test_run.joins_desktop() && test_run.may_join_running_window());
        assert!(!Cli::parse_from(["lumen400", "--info", "--desktop-integration"]).joins_desktop());
    }
}
