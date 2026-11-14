//! Video and audio frames with comprehensive SideData support (AVFrame reproduction).

use crate::buffer::BufferRef;
use crate::channel_layout::ChannelLayout;
use crate::color::{
    ChromaLocation, ColorPrimaries, ColorRange, ColorSpace, ColorTransferCharacteristic,
};
use crate::dict::Dictionary;
use crate::error::{Result, RmediaError};
use crate::pixfmt::PixelFormat;
use crate::rational::{Rational, NOPTS_VALUE};
use crate::samplefmt::SampleFormat;

/// Frame picture type for video (AVPictureType).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PictureType {
    #[default]
    None = 0,
    I = 1,
    P = 2,
    B = 3,
    S = 4,
    Si = 5,
    Sp = 6,
    Bi = 7,
}

/// Motion vector representation (AVMotionVector).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionVector {
    pub source: i32,
    pub w: u8,
    pub h: u8,
    pub src_x: i16,
    pub src_y: i16,
    pub dst_x: i16,
    pub dst_y: i16,
    pub flags: u64,
    pub motion_x: i32,
    pub motion_y: i32,
    pub motion_scale: u16,
}

/// ReplayGain audio volume metadata (AVReplayGain).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReplayGain {
    pub track_gain: i32,
    pub track_peak: u32,
    pub album_gain: i32,
    pub album_peak: u32,
}

/// Pan and Scan rectangle (AVPanScan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PanScan {
    pub width: i32,
    pub height: i32,
    pub position: [[i16; 2]; 3],
}

/// Spherical / 360 degree video metadata (AVSphericalMapping).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SphericalMetadata {
    pub projection: u32,
    pub yaw: i32,
    pub pitch: i32,
    pub roll: i32,
}

/// SMPTE 12M timecode representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SmpteTimecode {
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
    pub frames: u8,
    pub drop_frame: bool,
    pub fps: Rational,
}

/// Region of Interest (AVRegionOfInterest).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Roi {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
    pub qoffset: Rational,
}

/// Enumeration of frame side data types (AVFrameSideDataType).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FrameSideDataType {
    PanScan,
    A53ClosedCaptions,
    Stereo3d,
    MatrixEncoding,
    DownmixInfo,
    ReplayGain,
    DisplayMatrix,
    MotionVectors,
    MasteringDisplayMetadata,
    ContentLightLevel,
    IccProfile,
    SmpteTimecode,
    DolbyVisionRpu,
    SphericalMetadata,
    Roi,
    Custom(String),
}

/// Additional metadata attached to a frame (AVFrameSideData).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameSideData {
    pub side_type: FrameSideDataType,
    pub data: Vec<u8>,
}

impl FrameSideData {
    pub fn new(side_type: FrameSideDataType, data: Vec<u8>) -> Self {
        Self { side_type, data }
    }
}

/// Either pixel format or sample format depending on frame type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameFormat {
    #[default]
    None,
    Video(PixelFormat),
    Audio(SampleFormat),
}

/// Multimedia frame containing uncompressed video pixels or audio samples (AVFrame).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Pointers/buffers to the image/audio plane data.
    pub planes: [Option<BufferRef>; 8],
    /// For video: line size in bytes for each plane. For audio: size in bytes of each plane.
    pub linesize: [usize; 8],
    /// Video picture width in pixels.
    pub width: usize,
    /// Video picture height in pixels.
    pub height: usize,
    /// Number of audio samples per channel in this frame.
    pub nb_samples: usize,
    /// Pixel or sample format of the frame.
    pub format: FrameFormat,
    /// Flag indicating whether this is an intra/key frame.
    pub key_frame: bool,
    /// Picture type of the frame (I, P, B, etc.).
    pub pict_type: PictureType,
    /// Sample aspect ratio (SAR).
    pub sample_aspect_ratio: Rational,
    /// Presentation timestamp in time_base units.
    pub pts: i64,
    /// Decompression timestamp.
    pub pkt_dts: i64,
    /// Duration of the frame in time_base units.
    pub duration: i64,
    /// Timebase of the timestamps.
    pub time_base: Rational,
    /// Audio sample rate in Hz.
    pub sample_rate: u32,
    /// Audio channel layout.
    pub channel_layout: ChannelLayout,
    /// Video color range.
    pub color_range: ColorRange,
    /// Video color primaries.
    pub color_primaries: ColorPrimaries,
    /// Video transfer characteristic.
    pub color_trc: ColorTransferCharacteristic,
    /// Video color space.
    pub color_space: ColorSpace,
    /// Video chroma location.
    pub chroma_location: ChromaLocation,
    /// Frame side data elements.
    pub side_data: Vec<FrameSideData>,
    /// Metadata dictionary.
    pub metadata: Dictionary,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            planes: [None, None, None, None, None, None, None, None],
            linesize: [0; 8],
            width: 0,
            height: 0,
            nb_samples: 0,
            format: FrameFormat::None,
            key_frame: false,
            pict_type: PictureType::None,
            sample_aspect_ratio: Rational::new(1, 1),
            pts: NOPTS_VALUE,
            pkt_dts: NOPTS_VALUE,
            duration: 0,
            time_base: Rational::new(1, 1000),
            sample_rate: 0,
            channel_layout: ChannelLayout::default(),
            color_range: ColorRange::Unspecified,
            color_primaries: ColorPrimaries::Unspecified,
            color_trc: ColorTransferCharacteristic::Unspecified,
            color_space: ColorSpace::Unspecified,
            chroma_location: ChromaLocation::Unspecified,
            side_data: Vec::new(),
            metadata: Dictionary::new(),
        }
    }
}

impl Frame {
    /// Create a new empty video frame with specified dimensions and pixel format.
    pub fn new_video(width: usize, height: usize, pix_fmt: PixelFormat) -> Self {
        let mut frame = Self::default();
        frame.width = width;
        frame.height = height;
        frame.format = FrameFormat::Video(pix_fmt);
        frame
    }

    /// Allocate buffers for a video frame according to dimensions and pixel format.
    pub fn alloc_video(width: usize, height: usize, pix_fmt: PixelFormat, align: usize) -> Result<Self> {
        let mut frame = Self::new_video(width, height, pix_fmt);
        let desc = pix_fmt.descriptor().ok_or_else(|| {
            RmediaError::InvalidArgument(format!("Invalid pixel format {:?}", pix_fmt))
        })?;

        let align_val = if align == 0 { 64 } else { align };

        match pix_fmt {
            PixelFormat::Yuv420p | PixelFormat::Yuv422p | PixelFormat::Yuv444p => {
                let y_stride = (width + align_val - 1) & !(align_val - 1);
                let uv_width = width >> desc.log2_chroma_w;
                let uv_height = height >> desc.log2_chroma_h;
                let uv_stride = (uv_width + align_val - 1) & !(align_val - 1);

                frame.linesize[0] = y_stride;
                frame.linesize[1] = uv_stride;
                frame.linesize[2] = uv_stride;

                frame.planes[0] = Some(BufferRef::new(y_stride * height));
                frame.planes[1] = Some(BufferRef::new(uv_stride * uv_height));
                frame.planes[2] = Some(BufferRef::new(uv_stride * uv_height));
            }
            PixelFormat::Nv12 | PixelFormat::Nv21 => {
                let y_stride = (width + align_val - 1) & !(align_val - 1);
                let uv_stride = y_stride;
                let uv_height = height >> desc.log2_chroma_h;

                frame.linesize[0] = y_stride;
                frame.linesize[1] = uv_stride;

                frame.planes[0] = Some(BufferRef::new(y_stride * height));
                frame.planes[1] = Some(BufferRef::new(uv_stride * uv_height));
            }
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => {
                let stride = ((width * 3) + align_val - 1) & !(align_val - 1);
                frame.linesize[0] = stride;
                frame.planes[0] = Some(BufferRef::new(stride * height));
            }
            PixelFormat::Rgba | PixelFormat::Bgra | PixelFormat::Argb | PixelFormat::Abgr => {
                let stride = ((width * 4) + align_val - 1) & !(align_val - 1);
                frame.linesize[0] = stride;
                frame.planes[0] = Some(BufferRef::new(stride * height));
            }
            PixelFormat::Gray8 => {
                let stride = (width + align_val - 1) & !(align_val - 1);
                frame.linesize[0] = stride;
                frame.planes[0] = Some(BufferRef::new(stride * height));
            }
            _ => {
                let total_size = pix_fmt.image_buffer_size(width, height, align_val)?;
                frame.linesize[0] = total_size / height;
                frame.planes[0] = Some(BufferRef::new(total_size));
            }
        }

        Ok(frame)
    }

    /// Create a new empty audio frame with specified sample count and format.
    pub fn new_audio(nb_samples: usize, sample_rate: u32, channel_layout: ChannelLayout, sample_fmt: SampleFormat) -> Self {
        let mut frame = Self::default();
        frame.nb_samples = nb_samples;
        frame.sample_rate = sample_rate;
        frame.channel_layout = channel_layout;
        frame.format = FrameFormat::Audio(sample_fmt);
        frame
    }

    /// Allocate buffers for an audio frame.
    pub fn alloc_audio(
        nb_samples: usize,
        sample_rate: u32,
        channel_layout: ChannelLayout,
        sample_fmt: SampleFormat,
    ) -> Result<Self> {
        let mut frame = Self::new_audio(nb_samples, sample_rate, channel_layout, sample_fmt);
        let bytes_per_sample = sample_fmt.bytes_per_sample();
        let channels = channel_layout.nb_channels as usize;

        if sample_fmt.is_planar() {
            let plane_size = nb_samples * bytes_per_sample;
            for ch in 0..channels.min(8) {
                frame.linesize[ch] = plane_size;
                frame.planes[ch] = Some(BufferRef::new(plane_size));
            }
        } else {
            let total_size = nb_samples * channels * bytes_per_sample;
            frame.linesize[0] = total_size;
            frame.planes[0] = Some(BufferRef::new(total_size));
        }

        Ok(frame)
    }

    /// Add a side data entry to the frame.
    pub fn add_side_data(&mut self, side_type: FrameSideDataType, data: Vec<u8>) {
        // If one of the same type already exists, replace it
        if let Some(pos) = self.side_data.iter().position(|sd| sd.side_type == side_type) {
            self.side_data[pos] = FrameSideData::new(side_type, data);
        } else {
            self.side_data.push(FrameSideData::new(side_type, data));
        }
    }

    /// Get reference to a side data entry by type.
    pub fn get_side_data(&self, side_type: &FrameSideDataType) -> Option<&FrameSideData> {
        self.side_data.iter().find(|sd| &sd.side_type == side_type)
    }

    /// Ensure all plane buffers are uniquely owned (Copy-On-Write).
    pub fn make_writable(&mut self) {
        for plane in &mut self.planes {
            if let Some(buf) = plane {
                buf.make_writable();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_video_frame() {
        let f = Frame::alloc_video(640, 480, PixelFormat::Yuv420p, 64).unwrap();
        assert_eq!(f.width, 640);
        assert_eq!(f.height, 480);
        assert!(f.planes[0].is_some());
        assert!(f.planes[1].is_some());
        assert!(f.planes[2].is_some());
        assert_eq!(f.linesize[0], 640);
        assert_eq!(f.linesize[1], 320);
    }

    #[test]
    fn test_alloc_audio_frame() {
        let f = Frame::alloc_audio(1024, 48000, ChannelLayout::STEREO, SampleFormat::S16P).unwrap();
        assert_eq!(f.nb_samples, 1024);
        assert_eq!(f.linesize[0], 2048);
        assert_eq!(f.linesize[1], 2048);
        assert!(f.planes[0].is_some());
        assert!(f.planes[1].is_some());
    }
}
