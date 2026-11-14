//! Rmedia Swscale (`libswscale` reproduction in 100% pure Rust).
//!
//! Universal pixel format conversion, image scaling algorithms (Bilinear, Bicubic, Lanczos, etc.),
//! color matrix transformations (BT.601, BT.709, BT.2020), and color range mapping.

pub mod color_convert;
pub mod context;
pub mod scaler;

pub use color_convert::{rgb24_to_yuv420p, rgb_to_yuv, yuv420p_to_rgb24, yuv_to_rgb};
pub use context::SwsContext;
pub use scaler::{eval_filter, scale_plane_u8, ScalingAlgorithm};

#[cfg(test)]
mod tests {
    use super::*;
    use rmedia_util::frame::Frame;
    use rmedia_util::pixfmt::PixelFormat;

    #[test]
    fn test_sws_scale_yuv_resolution() {
        let ctx = SwsContext::get_context(
            64, 64, PixelFormat::Yuv420p,
            32, 32, PixelFormat::Yuv420p,
            ScalingAlgorithm::Bilinear,
        ).unwrap();

        let src = Frame::alloc_video(64, 64, PixelFormat::Yuv420p, 64).unwrap();
        let mut dst = Frame::alloc_video(32, 32, PixelFormat::Yuv420p, 64).unwrap();

        ctx.scale(&src, &mut dst).unwrap();
        assert_eq!(dst.width, 32);
        assert_eq!(dst.height, 32);
    }

    #[test]
    fn test_sws_color_conversion_yuv_to_rgb() {
        let ctx = SwsContext::get_context(
            16, 16, PixelFormat::Yuv420p,
            16, 16, PixelFormat::Rgb24,
            ScalingAlgorithm::Bilinear,
        ).unwrap();

        let src = Frame::alloc_video(16, 16, PixelFormat::Yuv420p, 64).unwrap();
        let mut dst = Frame::alloc_video(16, 16, PixelFormat::Rgb24, 64).unwrap();

        ctx.scale(&src, &mut dst).unwrap();
        assert_eq!(dst.width, 16);
        assert_eq!(dst.height, 16);
    }
}
