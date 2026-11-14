//! SwsContext scaling and pixel conversion coordinator (libswscale reproduction).

use crate::color_convert::{rgb24_to_yuv420p, yuv420p_to_rgb24};
use crate::scaler::{scale_plane_u8, ScalingAlgorithm};
use rmedia_util::color::{ColorMatrixCoeffs, ColorRange, ColorSpace};
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::pixfmt::PixelFormat;

/// Image scaling and pixel conversion context (AVSwsContext).
#[derive(Debug, Clone)]
pub struct SwsContext {
    pub src_w: usize,
    pub src_h: usize,
    pub src_format: PixelFormat,
    pub dst_w: usize,
    pub dst_h: usize,
    pub dst_format: PixelFormat,
    pub algorithm: ScalingAlgorithm,
    pub color_space: ColorSpace,
    pub color_range: ColorRange,
}

impl SwsContext {
    /// Create a new scaling and conversion context.
    pub fn get_context(
        src_w: usize,
        src_h: usize,
        src_format: PixelFormat,
        dst_w: usize,
        dst_h: usize,
        dst_format: PixelFormat,
        algorithm: ScalingAlgorithm,
    ) -> Result<Self> {
        if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
            return Err(RmediaError::InvalidArgument("Dimensions must be positive".into()));
        }

        Ok(Self {
            src_w,
            src_h,
            src_format,
            dst_w,
            dst_h,
            dst_format,
            algorithm,
            color_space: ColorSpace::Bt709,
            color_range: ColorRange::Limited,
        })
    }

    /// Perform scaling and format conversion from source frame to destination frame.
    pub fn scale(&self, src: &Frame, dst: &mut Frame) -> Result<()> {
        let coeffs = ColorMatrixCoeffs::for_space(self.color_space);

        if self.src_format == PixelFormat::Yuv420p && self.dst_format == PixelFormat::Yuv420p {
            // Planar YUV scaling
            let src_y = src.planes[0].as_ref().unwrap().as_slice();
            let src_u = src.planes[1].as_ref().unwrap().as_slice();
            let src_v = src.planes[2].as_ref().unwrap().as_slice();

            let [ref mut p0, ref mut p1, ref mut p2, ..] = dst.planes;
            let dst_y = p0.as_mut().unwrap().as_mut_slice();
            let dst_u = p1.as_mut().unwrap().as_mut_slice();
            let dst_v = p2.as_mut().unwrap().as_mut_slice();

            scale_plane_u8(src_y, src.linesize[0], self.src_w, self.src_h, dst_y, dst.linesize[0], self.dst_w, self.dst_h, self.algorithm);
            scale_plane_u8(src_u, src.linesize[1], self.src_w / 2, self.src_h / 2, dst_u, dst.linesize[1], self.dst_w / 2, self.dst_h / 2, self.algorithm);
            scale_plane_u8(src_v, src.linesize[2], self.src_w / 2, self.src_h / 2, dst_v, dst.linesize[2], self.dst_w / 2, self.dst_h / 2, self.algorithm);
        } else if self.src_format == PixelFormat::Yuv420p && self.dst_format == PixelFormat::Rgb24 {
            // First scale YUV planes to dst dimensions if needed
            let mut tmp_dst = Frame::alloc_video(self.dst_w, self.dst_h, PixelFormat::Yuv420p, 64)?;
            let src_y = src.planes[0].as_ref().unwrap().as_slice();
            let src_u = src.planes[1].as_ref().unwrap().as_slice();
            let src_v = src.planes[2].as_ref().unwrap().as_slice();

            let [ref mut tp0, ref mut tp1, ref mut tp2, ..] = tmp_dst.planes;
            let tmp_y = tp0.as_mut().unwrap().as_mut_slice();
            let tmp_u = tp1.as_mut().unwrap().as_mut_slice();
            let tmp_v = tp2.as_mut().unwrap().as_mut_slice();

            scale_plane_u8(src_y, src.linesize[0], self.src_w, self.src_h, tmp_y, tmp_dst.linesize[0], self.dst_w, self.dst_h, self.algorithm);
            scale_plane_u8(src_u, src.linesize[1], self.src_w / 2, self.src_h / 2, tmp_u, tmp_dst.linesize[1], self.dst_w / 2, self.dst_h / 2, self.algorithm);
            scale_plane_u8(src_v, src.linesize[2], self.src_w / 2, self.src_h / 2, tmp_v, tmp_dst.linesize[2], self.dst_w / 2, self.dst_h / 2, self.algorithm);

            // Convert to RGB24
            let dst_rgb = dst.planes[0].as_mut().unwrap().as_mut_slice();
            yuv420p_to_rgb24(
                tmp_dst.planes[0].as_ref().unwrap().as_slice(),
                tmp_dst.linesize[0],
                tmp_dst.planes[1].as_ref().unwrap().as_slice(),
                tmp_dst.linesize[1],
                tmp_dst.planes[2].as_ref().unwrap().as_slice(),
                tmp_dst.linesize[2],
                self.dst_w,
                self.dst_h,
                dst_rgb,
                dst.linesize[0],
                coeffs,
                self.color_range,
            );
        } else if self.src_format == PixelFormat::Rgb24 && self.dst_format == PixelFormat::Yuv420p {
            // First convert RGB24 to YUV420P at source resolution
            let mut tmp_src = Frame::alloc_video(self.src_w, self.src_h, PixelFormat::Yuv420p, 64)?;
            let src_rgb = src.planes[0].as_ref().unwrap().as_slice();
            {
                let [ref mut sp0, ref mut sp1, ref mut sp2, ..] = tmp_src.planes;
                rgb24_to_yuv420p(
                    src_rgb,
                    src.linesize[0],
                    self.src_w,
                    self.src_h,
                    sp0.as_mut().unwrap().as_mut_slice(),
                    tmp_src.linesize[0],
                    sp1.as_mut().unwrap().as_mut_slice(),
                    tmp_src.linesize[1],
                    sp2.as_mut().unwrap().as_mut_slice(),
                    tmp_src.linesize[2],
                    coeffs,
                    self.color_range,
                );
            }

            // Scale to destination resolution
            let tmp_y = tmp_src.planes[0].as_ref().unwrap().as_slice();
            let tmp_u = tmp_src.planes[1].as_ref().unwrap().as_slice();
            let tmp_v = tmp_src.planes[2].as_ref().unwrap().as_slice();

            let [ref mut dp0, ref mut dp1, ref mut dp2, ..] = dst.planes;
            let dst_y = dp0.as_mut().unwrap().as_mut_slice();
            let dst_u = dp1.as_mut().unwrap().as_mut_slice();
            let dst_v = dp2.as_mut().unwrap().as_mut_slice();

            scale_plane_u8(tmp_y, tmp_src.linesize[0], self.src_w, self.src_h, dst_y, dst.linesize[0], self.dst_w, self.dst_h, self.algorithm);
            scale_plane_u8(tmp_u, tmp_src.linesize[1], self.src_w / 2, self.src_h / 2, dst_u, dst.linesize[1], self.dst_w / 2, self.dst_h / 2, self.algorithm);
            scale_plane_u8(tmp_v, tmp_src.linesize[2], self.src_w / 2, self.src_h / 2, dst_v, dst.linesize[2], self.dst_w / 2, self.dst_h / 2, self.algorithm);
        } else {
            // Direct plane copy if identical
            if let (Some(s), Some(d)) = (src.planes[0].as_ref(), dst.planes[0].as_mut()) {
                let copy_len = s.len().min(d.len());
                d.as_mut_slice()[..copy_len].copy_from_slice(&s.as_slice()[..copy_len]);
            }
        }

        dst.width = self.dst_w;
        dst.height = self.dst_h;
        dst.format = FrameFormat::Video(self.dst_format);
        dst.pts = src.pts;
        Ok(())
    }
}
