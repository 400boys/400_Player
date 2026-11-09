//! Everything that touches the file system on behalf of the interface: native file dialogs (run on
//! a helper thread so the window never freezes), screenshots, "show in folder".

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use lumen400_core::media_kind::{AUDIO_EXTS, PLAYLIST_EXTS, SUBTITLE_EXTS, VIDEO_EXTS};

/// An answer from a helper thread.
#[derive(Debug)]
pub enum Answer {
    /// Files chosen in the "open" dialog. `append`: add to the playlist instead of replacing it.
    Files {
        paths: Vec<PathBuf>,
        append: bool,
    },
    Folder {
        path: PathBuf,
        append: bool,
    },
    Subtitle(PathBuf),
    LoadPlaylist(PathBuf),
    SavePlaylist(PathBuf),
    ScreenshotFolder(PathBuf),
    ScreenshotSaved(PathBuf),
    ScreenshotFailed(String),
}

/// Starts dialogs and collects what they return.
pub struct Dialogs {
    tx: Sender<Answer>,
    rx: Receiver<Answer>,
    ctx: egui::Context,
}

impl Dialogs {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, ctx }
    }

    /// Everything that arrived since the last call.
    pub fn poll(&self) -> Vec<Answer> {
        self.rx.try_iter().collect()
    }

    /// A sender for helper threads of other kinds (screenshot encoding).
    pub fn sender(&self) -> (Sender<Answer>, egui::Context) {
        (self.tx.clone(), self.ctx.clone())
    }

    fn spawn(&self, work: impl FnOnce() -> Option<Answer> + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let _ = std::thread::Builder::new().name("lumen400-dialog".into()).spawn(move || {
            if let Some(answer) = work() {
                let _ = tx.send(answer);
                ctx.request_repaint();
            }
        });
    }

    pub fn open_files(&self, start_dir: Option<String>, append: bool) {
        self.spawn(move || {
            let mut d = rfd::AsyncFileDialog::new()
                .set_title(crate::i18n::t(crate::i18n::Key::OpenFiles))
                .add_filter(crate::i18n::t(crate::i18n::Key::FilterMedia), &media_extensions())
                .add_filter(crate::i18n::t(crate::i18n::Key::FilterVideo), VIDEO_EXTS)
                .add_filter(crate::i18n::t(crate::i18n::Key::FilterAudio), AUDIO_EXTS)
                .add_filter(crate::i18n::t(crate::i18n::Key::FilterPlaylist), PLAYLIST_EXTS);
            if let Some(dir) = start_dir {
                d = d.set_directory(dir);
            }
            let picked = pollster::block_on(d.pick_files())?;
            let paths: Vec<PathBuf> = picked.iter().map(|f| f.path().to_path_buf()).collect();
            (!paths.is_empty()).then_some(Answer::Files { paths, append })
        });
    }

    pub fn open_folder(&self, start_dir: Option<String>, append: bool) {
        self.spawn(move || {
            let mut d = rfd::AsyncFileDialog::new().set_title(crate::i18n::t(crate::i18n::Key::OpenFolder));
            if let Some(dir) = start_dir {
                d = d.set_directory(dir);
            }
            let f = pollster::block_on(d.pick_folder())?;
            Some(Answer::Folder { path: f.path().to_path_buf(), append })
        });
    }

    pub fn open_subtitle(&self, start_dir: Option<String>) {
        self.spawn(move || {
            let mut d =
                rfd::AsyncFileDialog::new().set_title(crate::i18n::t(crate::i18n::Key::LoadSubtitles)).add_filter(crate::i18n::t(crate::i18n::Key::FilterSubtitles), SUBTITLE_EXTS);
            if let Some(dir) = start_dir {
                d = d.set_directory(dir);
            }
            let f = pollster::block_on(d.pick_file())?;
            Some(Answer::Subtitle(f.path().to_path_buf()))
        });
    }

    pub fn load_playlist(&self, start_dir: Option<String>) {
        self.spawn(move || {
            let mut d =
                rfd::AsyncFileDialog::new().set_title(crate::i18n::t(crate::i18n::Key::LoadPlaylist)).add_filter(crate::i18n::t(crate::i18n::Key::FilterPlaylist), PLAYLIST_EXTS);
            if let Some(dir) = start_dir {
                d = d.set_directory(dir);
            }
            let f = pollster::block_on(d.pick_file())?;
            Some(Answer::LoadPlaylist(f.path().to_path_buf()))
        });
    }

    pub fn save_playlist(&self, start_dir: Option<String>) {
        self.spawn(move || {
            let mut d = rfd::AsyncFileDialog::new()
                .set_title(crate::i18n::t(crate::i18n::Key::SavePlaylist))
                .set_file_name("playlist.m3u8")
                .add_filter("M3U", &["m3u8", "m3u"])
                .add_filter("XSPF", &["xspf"]);
            if let Some(dir) = start_dir {
                d = d.set_directory(dir);
            }
            let f = pollster::block_on(d.save_file())?;
            Some(Answer::SavePlaylist(f.path().to_path_buf()))
        });
    }

    pub fn pick_screenshot_folder(&self, start_dir: Option<PathBuf>) {
        self.spawn(move || {
            let mut d = rfd::AsyncFileDialog::new().set_title(crate::i18n::t(crate::i18n::Key::ScreenshotFolder));
            if let Some(dir) = start_dir {
                d = d.set_directory(dir);
            }
            let f = pollster::block_on(d.pick_folder())?;
            Some(Answer::ScreenshotFolder(f.path().to_path_buf()))
        });
    }
}

/// Every extension the player can open.
pub fn media_extensions() -> Vec<&'static str> {
    VIDEO_EXTS.iter().chain(AUDIO_EXTS.iter()).copied().collect()
}

/// A file name that is safe on every platform, built from a media title.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let short: String = trimmed.chars().take(80).collect();
    if short.is_empty() { "400_Player".to_owned() } else { short }
}

/// First free `dir/stem.png`, `dir/stem (2).png`…
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..10_000).map(|n| dir.join(format!("{stem} ({n}).{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// Encodes an RGBA picture as PNG on a helper thread and reports the result.
pub fn save_png(dialogs: &Dialogs, dir: PathBuf, stem: String, rgba: Vec<u8>, width: u32, height: u32) {
    let (tx, ctx) = dialogs.sender();
    let _ = std::thread::Builder::new().name("lumen400-png".into()).spawn(move || {
        let result = (|| -> Result<PathBuf, String> {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = unique_path(&dir, &sanitize_file_name(&stem), "png");
            let img = image::RgbaImage::from_raw(width, height, rgba).ok_or_else(|| "bad picture size".to_owned())?;
            img.save_with_format(&path, image::ImageFormat::Png).map_err(|e| e.to_string())?;
            Ok(path)
        })();
        let _ = tx.send(match result {
            Ok(p) => Answer::ScreenshotSaved(p),
            Err(e) => Answer::ScreenshotFailed(e),
        });
        ctx.request_repaint();
    });
}

/// Shows `path` in the system file manager.
pub fn reveal(path: &Path) {
    use std::process::Command;
    let result = if cfg!(target_os = "windows") {
        Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg("-R").arg(path).spawn()
    } else {
        let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        Command::new("xdg-open").arg(dir).spawn()
    };
    if let Err(e) = result {
        tracing::warn!("cannot show {} in the file manager: {e}", path.display());
    }
}

/// Opens a folder in the system file manager.
pub fn open_dir(dir: &Path) {
    reveal(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_portable() {
        assert_eq!(sanitize_file_name("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize_file_name("  ..hidden.. "), "hidden");
        assert_eq!(sanitize_file_name(""), "400_Player");
        assert_eq!(sanitize_file_name(&"x".repeat(200)).chars().count(), 80);
        assert_eq!(sanitize_file_name("Été – Ça va ?"), "Été – Ça va _");
    }

    #[test]
    fn unique_paths_do_not_overwrite() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-files")
            .join(format!("lumen400-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = unique_path(&dir, "shot", "png");
        assert_eq!(a, dir.join("shot.png"));
        std::fs::write(&a, b"x").unwrap();
        let b = unique_path(&dir, "shot", "png");
        assert_eq!(b, dir.join("shot (2).png"));
        std::fs::write(&b, b"x").unwrap();
        assert_eq!(unique_path(&dir, "shot", "png"), dir.join("shot (3).png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn media_extension_list_covers_common_formats() {
        let all = media_extensions();
        for e in ["mp4", "mkv", "webm", "mp3", "flac"] {
            assert!(all.contains(&e), "{e}");
        }
    }
}
