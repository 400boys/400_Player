//! Headless audio and video device sinks and generators for CI/servers and media players.

use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::Result;
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::pixfmt::PixelFormat;
use rmedia_util::samplefmt::SampleFormat;

/// Headless audio output sink (collects played audio samples).
#[derive(Debug, Clone, Default)]
pub struct HeadlessAudioSink {
    pub total_samples: usize,
    pub sample_rate: u32,
    pub channels: usize,
    pub peak_amplitude: f32,
}

impl HeadlessAudioSink {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            total_samples: 0,
            sample_rate,
            channels,
            peak_amplitude: 0.0,
        }
    }

    /// Play an audio frame through the sink.
    pub fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        self.total_samples += frame.nb_samples;
        if let Some(ref buf) = frame.planes[0] {
            let slice = buf.as_slice();
            let count = slice.len() / 2;
            for i in 0..count {
                let s = i16::from_le_bytes(slice[i * 2..i * 2 + 2].try_into().unwrap());
                let amp = (s as f32).abs() / 32768.0;
                if amp > self.peak_amplitude {
                    self.peak_amplitude = amp;
                }
            }
        }
        Ok(())
    }
}

/// Headless video output sink (collects displayed frames into virtual framebuffer).
#[derive(Debug, Clone, Default)]
pub struct HeadlessVideoSink {
    pub total_frames: usize,
    pub width: usize,
    pub height: usize,
    pub last_frame_hash: u32,
}

impl HeadlessVideoSink {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            total_frames: 0,
            width,
            height,
            last_frame_hash: 0,
        }
    }

    /// Display a video frame on the virtual screen.
    pub fn render_frame(&mut self, frame: &Frame) -> Result<()> {
        self.total_frames += 1;
        self.width = frame.width;
        self.height = frame.height;

        if let Some(ref buf) = frame.planes[0] {
            self.last_frame_hash = rmedia_util::crypto::crc32_ieee(buf.as_slice());
        }

        Ok(())
    }
}

/// SMPTE color bars test generator.
pub fn generate_smpte_bars_yuv420p(width: usize, height: usize) -> Result<Frame> {
    let mut frame = Frame::alloc_video(width, height, PixelFormat::Yuv420p, 64)?;
    // 7 standard colors: Gray, Yellow, Cyan, Green, Magenta, Red, Blue
    let colors = [
        (180u8, 128u8, 128u8), // Gray
        (162, 44, 142),        // Yellow
        (131, 156, 44),        // Cyan
        (112, 72, 58),         // Green
        (84, 184, 198),        // Magenta
        (65, 100, 212),        // Red
        (35, 212, 114),        // Blue
    ];

    let [ref mut p0, ref mut p1, ref mut p2, ..] = frame.planes;
    let y_plane = p0.as_mut().unwrap().as_mut_slice();
    let u_plane = p1.as_mut().unwrap().as_mut_slice();
    let v_plane = p2.as_mut().unwrap().as_mut_slice();

    let y_stride = frame.linesize[0];
    let u_stride = frame.linesize[1];
    let v_stride = frame.linesize[2];

    let bar_w = width / 7;

    for y in 0..height {
        for x in 0..width {
            let color_idx = (x / bar_w.max(1)).min(6);
            let (y_val, u_val, v_val) = colors[color_idx];

            y_plane[y * y_stride + x] = y_val;
            if y % 2 == 0 && x % 2 == 0 {
                let uv_offset = (y / 2) * u_stride + (x / 2);
                u_plane[uv_offset] = u_val;
                v_plane[uv_offset] = v_val;
            }
        }
    }

    Ok(frame)
}
