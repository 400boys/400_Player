//! Lumen400 — a modern, GPU-accelerated, low-latency media player.

#![forbid(unsafe_code)]
#![windows_subsystem = "windows"]

mod actions;
mod app;
mod automation;
mod cli;
mod control;
mod desktop;
mod files;
mod fonts;
mod i18n;
mod power;
mod state;
mod theme;
mod ui;
mod util;
mod video;
mod widgets;

use std::sync::Arc;

use anyhow::{Context as _, Result};
use clap::Parser;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use lumen400_core::settings::Settings;
use lumen400_engine::LangPrefs;

use crate::app::LumenApp;
use crate::cli::Cli;

/// Application id: window class on X11, app id on Wayland, desktop-file name on Linux.
pub const APP_ID: &str = "lumen400";

fn main() -> Result<()> {
    install_panic_hook();
    if let Err(err) = run_main() {
        let msg = format!("400_Player startup error: {err:#}\n");
        eprintln!("\n============================================================================");
        eprintln!("{msg}");
        eprintln!("============================================================================");
        write_crash_log(&msg);
        pause_console_on_error();
        return Err(err);
    }
    Ok(())
}

fn write_crash_log(msg: &str) {
    let _ = std::fs::write("400_Player-crash.log", msg);
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let _ = std::fs::write(dir.join("400_Player-crash.log"), msg);
    }
}

fn pause_console_on_error() {
    if cfg!(target_os = "windows") {
        eprintln!("\n[400_Player] Une erreur est survenue. Appuyez sur Entree pour fermer cette fenetre...");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!("400_Player panic: {info}\nBacktrace:\n{bt}\n");
        eprintln!("\n============================================================================");
        eprintln!("{msg}");
        eprintln!("============================================================================");
        write_crash_log(&msg);
        default_hook(info);
    }));
}

fn run_main() -> Result<()> {
    let cli = Cli::parse();
    init_logging();
    eprintln!("[400_Player] Demarrage de 400_Player v{} (100% Pur Rust, 0 DLL)...", env!("CARGO_PKG_VERSION"));
    lumen400_engine::init();

    if cli.info {
        return print_info(&cli);
    }

    let ephemeral = cli.ephemeral;
    let mut settings = if ephemeral { Settings::default() } else { Settings::load() };
    if let Some(t) = cli.theme {
        settings.theme = t.into();
    }
    if let Some(a) = cli.accent {
        settings.accent = a.into();
    }
    if let Some(l) = cli.lang {
        settings.language = l.into();
    }

    // One window: a launch while Lumen400 already runs goes to that window and ends here.
    let mut service = None;
    if cli.joins_desktop() {
        if settings.single_instance && cli.may_join_running_window() {
            match desktop::hand_over(&cli) {
                desktop::HandOver::Delivered => {
                    tracing::info!("handed over to the window that is already open");
                    return Ok(());
                }
                desktop::HandOver::NoRunningWindow => {}
                desktop::HandOver::Failed(why) => tracing::warn!("the open window did not take the request ({why}): opening a new one"),
            }
        }
        service = desktop::Service::start();
    }

    eprintln!("[400_Player] Initialisation de la fenetre graphique et du GPU (WGPU)...");
    let options = native_options(&settings);
    eframe::run_native("400_Player", options, Box::new(move |cc| Ok(Box::new(LumenApp::new(cc, &cli, settings, ephemeral, service)))))
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("the window could not be created (is a graphics driver with Vulkan, Metal or DirectX 12 installed?)")?;
    // A script run (`--automation`) reports its failed expectations through the exit code.
    let code = automation::EXIT_CODE.load(std::sync::atomic::Ordering::Relaxed);
    if code != 0 {
        std::process::exit(code.clamp(1, 100));
    }
    Ok(())
}

fn init_logging() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,lumen400=info,lumen400_engine=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_target(false).compact().try_init();
}

/// `--info`: what the pure-Rust engine finds in each input, as JSON.
fn print_info(cli: &Cli) -> Result<()> {
    if cli.inputs.is_empty() {
        anyhow::bail!("--info needs at least one file or URL");
    }
    let prefs = LangPrefs::default();
    let mut all = Vec::new();
    for input in &cli.inputs {
        let info = lumen400_engine::probe(input, &prefs).with_context(|| format!("cannot open {input}"))?;
        all.push(info);
    }
    let json = if all.len() == 1 { serde_json::to_string_pretty(&all[0])? } else { serde_json::to_string_pretty(&all)? };
    println!("{json}");
    Ok(())
}

/// The window icon.
fn window_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../assets/icon/400player-256.png");
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

fn native_options(settings: &Settings) -> eframe::NativeOptions {
    let w = &settings.window;
    let width = w.width.clamp(640.0, 1920.0);
    let height = w.height.clamp(400.0, 1080.0);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("400_Player")
        .with_app_id(APP_ID)
        .with_inner_size([width, height])
        .with_min_inner_size([480.0, 320.0])
        .with_maximized(w.maximized)
        .with_visible(true)
        .with_decorations(!settings.custom_titlebar)
        .with_drag_and_drop(true);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let mut setup = WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = eframe::wgpu::Backends::from_env().unwrap_or(if cfg!(target_os = "windows") {
        eframe::wgpu::Backends::DX12
    } else {
        eframe::wgpu::Backends::PRIMARY
    });
    setup.power_preference = eframe::wgpu::PowerPreference::from_env().unwrap_or_default();
    let base = setup.device_descriptor.clone();
    setup.device_descriptor = Arc::new(move |adapter| {
        let mut desc = base(adapter);
        desc.label = Some("lumen400 device");
        if adapter.features().contains(eframe::wgpu::Features::TEXTURE_FORMAT_16BIT_NORM) {
            desc.required_features |= eframe::wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
        }
        desc
    });

    eframe::NativeOptions {
        viewport,
        wgpu_options: WgpuConfiguration { wgpu_setup: WgpuSetup::CreateNew(setup), ..Default::default() },
        dithering: false,
        ..Default::default()
    }
}
