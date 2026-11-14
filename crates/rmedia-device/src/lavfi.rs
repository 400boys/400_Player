//! Virtual libavfilter input device (`lavfi`).

use rmedia_codec::codec_id::MediaType;
use rmedia_format::format_context::{Demuxer, FormatContext};
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;
use rmedia_util::rational::Rational;
use rmedia_util::samplefmt::SampleFormat;

/// Virtual filtergraph source input device (e.g. -f lavfi -i testsrc).
#[derive(Debug, Clone, Default)]
pub struct LavfiDevice {
    graph_source: String,
    frame_count: i64,
    media_type: MediaType,
    width: usize,
    height: usize,
}

impl LavfiDevice {
    pub fn new(source_expr: &str) -> Self {
        let (mtype, w, h) = if source_expr.contains("sine") || source_expr.contains("anoise") {
            (MediaType::Audio, 0, 0)
        } else {
            (MediaType::Video, 640, 480)
        };

        Self {
            graph_source: source_expr.to_string(),
            frame_count: 0,
            media_type: mtype,
            width: w,
            height: h,
        }
    }
}

impl Demuxer for LavfiDevice {
    fn name(&self) -> &'static str {
        "lavfi"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let codec_id = if self.media_type == MediaType::Audio {
            rmedia_codec::codec_id::CodecId::PcmS16le
        } else {
            rmedia_codec::codec_id::CodecId::RawVideo
        };

        let s_idx = ctx.new_stream(codec_id);
        let s = ctx.stream_mut(s_idx).unwrap();
        if self.media_type == MediaType::Audio {
            s.time_base = Rational::new(1, 44100);
            s.codec_context.sample_rate = 44100;
            s.codec_context.ch_layout = ChannelLayout::STEREO;
            s.codec_context.sample_fmt = SampleFormat::S16;
        } else {
            s.time_base = Rational::new(1, 25);
            s.avg_frame_rate = Rational::new(25, 1);
            s.r_frame_rate = s.avg_frame_rate;
            s.codec_context.width = self.width;
            s.codec_context.height = self.height;
            s.codec_context.pix_fmt = PixelFormat::Yuv420p;
        }

        self.frame_count = 0;
        Ok(())
    }

    fn read_packet(&mut self, _ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        if self.media_type == MediaType::Audio {
            // Generate 1024 samples of 440 Hz test sine wave
            let nb_samples = 1024;
            let mut buf = vec![0u8; nb_samples * 4]; // 2 channels * 16-bit
            for i in 0..nb_samples {
                let t = (self.frame_count * 1024 + i as i64) as f32 / 44100.0;
                let sample = ((t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 16000.0) as i16;
                let bytes = sample.to_le_bytes();
                buf[i * 4..i * 4 + 2].copy_from_slice(&bytes);
                buf[i * 4 + 2..i * 4 + 4].copy_from_slice(&bytes);
            }

            *pkt = Packet::from_slice(&buf);
            pkt.pts = self.frame_count * 1024;
            pkt.dts = pkt.pts;
        } else {
            // Generate synthetic color frame
            let size = PixelFormat::Yuv420p.image_buffer_size(self.width, self.height, 1)?;
            let mut buf = vec![128u8; size];
            // Animate Y luminance
            let lum = (((self.frame_count * 5) % 200) + 30) as u8;
            buf[..self.width * self.height].fill(lum);

            *pkt = Packet::from_slice(&buf);
            pkt.pts = self.frame_count;
            pkt.dts = self.frame_count;
        }

        pkt.stream_index = 0;
        pkt.set_keyframe(true);
        self.frame_count += 1;
        Ok(())
    }

    fn seek(&mut self, _ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        self.frame_count = timestamp.max(0);
        Ok(())
    }
}
