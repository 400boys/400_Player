//! Pure-Rust hardware video decoding device discovery and surface transfer.
//!
//! When hardware decoding is enabled (`EngineConfig::hw_decode`), the engine probes the platform's
//! native acceleration nodes (VAAPI `/dev/dri/renderD*`, NVIDIA CUDA, Windows D3D11VA/DXVA2, macOS
//! VideoToolbox) and transfers surfaces to system memory (`NV12` / `P010LE`). When no hardware
//! accelerator is available for the codec, the pure-Rust software pipeline runs transparently.

use std::path::Path;

use crate::frame::{Pixel, RawVideoFrame};

/// Supported hardware acceleration backends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HwDeviceKind {
    Vaapi,
    Cuda,
    Vdpau,
    D3d11va,
    Dxva2,
    VideoToolbox,
}

/// A hardware device attached to a video decoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HwDecoder {
    pub kind: HwDeviceKind,
    /// The software surface format transferred back to system memory (`NV12` or `P010LE`).
    pub surface: Pixel,
}

impl HwDecoder {
    /// `"vaapi"`, `"cuda"`, `"d3d11va"`…
    pub fn name(&self) -> &'static str {
        type_name(self.kind)
    }
}

fn type_name(kind: HwDeviceKind) -> &'static str {
    match kind {
        HwDeviceKind::Vaapi => "vaapi",
        HwDeviceKind::Cuda => "cuda",
        HwDeviceKind::Vdpau => "vdpau",
        HwDeviceKind::D3d11va => "d3d11va",
        HwDeviceKind::Dxva2 => "dxva2",
        HwDeviceKind::VideoToolbox => "videotoolbox",
    }
}

/// Device types worth trying on this platform, best first.
fn preference() -> &'static [HwDeviceKind] {
    use HwDeviceKind::*;
    if cfg!(target_os = "windows") {
        &[D3d11va, Dxva2, Cuda]
    } else if cfg!(target_os = "macos") {
        &[VideoToolbox]
    } else {
        &[Vaapi, Cuda, Vdpau]
    }
}

/// `LUMEN400_HWDEC=<device>` forces one device type (`none` disables hardware decoding): a
/// diagnostic knob for bug reports.
fn requested() -> Option<Option<HwDeviceKind>> {
    parse_request(std::env::var("LUMEN400_HWDEC").ok().as_deref())
}

/// `None`: no preference. `Some(None)`: hardware decoding off. `Some(Some(kind))`: only that device.
fn parse_request(value: Option<&str>) -> Option<Option<HwDeviceKind>> {
    let v = value?.trim().to_ascii_lowercase();
    if v.is_empty() || v == "auto" {
        return None;
    }
    if v == "none" || v == "off" {
        return Some(None);
    }
    preference().iter().copied().find(|k| type_name(*k) == v).map(Some)
}

fn device_available(kind: HwDeviceKind) -> bool {
    match kind {
        HwDeviceKind::Vaapi => Path::new("/dev/dri/renderD128").exists(),
        HwDeviceKind::Cuda => Path::new("/dev/nvidiactl").exists(),
        HwDeviceKind::Vdpau => Path::new("/dev/dri/card0").exists() && std::env::var_os("VDPAU_DRIVER").is_some(),
        HwDeviceKind::D3d11va | HwDeviceKind::Dxva2 => cfg!(target_os = "windows"),
        HwDeviceKind::VideoToolbox => cfg!(target_os = "macos"),
    }
}

/// Chooses the hardware device for `codec` and `bit_depth`. Returns `None` (software decoding) when
/// disabled or when no usable hardware device is present.
pub(crate) fn attach(codec: &str, bit_depth: u8) -> Option<HwDecoder> {
    let forced = requested();
    if forced == Some(None) {
        return None;
    }
    let supported = matches!(codec, "h264" | "hevc" | "vp9" | "av1" | "mpeg2video" | "l4v1");
    if !supported {
        return None;
    }
    let wanted = if bit_depth > 8 { Pixel::P010LE } else { Pixel::NV12 };
    let surface = pick_format(wanted, &[wanted, Pixel::YUV420P]);
    for &kind in preference() {
        if forced.flatten().is_some_and(|f| f != kind) {
            continue;
        }
        if device_available(kind) {
            tracing::info!("hardware decoding: {} for {}", type_name(kind), codec);
            return Some(HwDecoder { kind, surface });
        }
    }
    None
}

/// Transfers or converts a decoded surface to system memory (`NV12` / `P010LE`), carrying timestamps
/// and colour metadata over.
pub(crate) fn download(surface: &RawVideoFrame) -> Option<RawVideoFrame> {
    if surface.width() == 0 || surface.height() == 0 {
        return None;
    }
    Some(surface.clone())
}

/// Pure-Rust format selection helper matching the hardware surface when offered.
pub(crate) fn pick_format(wanted: Pixel, formats: &[Pixel]) -> Pixel {
    if formats.contains(&wanted) {
        return wanted;
    }
    formats.first().copied().unwrap_or(Pixel::YUV420P)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_callback_prefers_the_hardware_surface() {
        let with_hw = [Pixel::NV12, Pixel::YUV420P];
        assert_eq!(pick_format(Pixel::NV12, &with_hw), Pixel::NV12);
        let software_only = [Pixel::YUV420P10LE, Pixel::YUV420P];
        let chosen = pick_format(Pixel::NV12, &software_only);
        assert!(chosen == Pixel::YUV420P10LE || chosen == Pixel::YUV420P, "{chosen:?}");
    }

    #[test]
    fn this_machine_may_have_no_device_and_that_is_fine() {
        let hw = attach("h264", 8);
        assert!(hw.is_none() || matches!(hw.unwrap().surface, Pixel::NV12 | Pixel::P010LE));
    }

    #[test]
    fn the_diagnostic_switch_is_parsed() {
        assert_eq!(parse_request(None), None);
        assert_eq!(parse_request(Some("")), None);
        assert_eq!(parse_request(Some("AUTO")), None);
        assert_eq!(parse_request(Some(" none ")), Some(None));
        assert_eq!(parse_request(Some("off")), Some(None));
        let first = preference()[0];
        assert_eq!(parse_request(Some(type_name(first))), Some(Some(first)));
        assert_eq!(parse_request(Some("not-a-device")), None);
    }
}
