//! Audio sample format conversion and dithering.

use rmedia_util::samplefmt::SampleFormat;

/// Convert raw bytes of any supported sample format into normalized f32 samples.
pub fn bytes_to_f32(src: &[u8], fmt: SampleFormat, dst: &mut [f32]) {
    match fmt {
        SampleFormat::U8 | SampleFormat::U8P => {
            for (i, &b) in src.iter().enumerate().take(dst.len()) {
                dst[i] = ((b as f32) - 128.0) / 128.0;
            }
        }
        SampleFormat::S16 | SampleFormat::S16P => {
            let count = (src.len() / 2).min(dst.len());
            for i in 0..count {
                let s = i16::from_le_bytes(src[i * 2..i * 2 + 2].try_into().unwrap());
                dst[i] = (s as f32) / 32768.0;
            }
        }
        SampleFormat::S32 | SampleFormat::S32P => {
            let count = (src.len() / 4).min(dst.len());
            for i in 0..count {
                let s = i32::from_le_bytes(src[i * 4..i * 4 + 4].try_into().unwrap());
                dst[i] = (s as f32) / 2147483648.0;
            }
        }
        SampleFormat::Flt | SampleFormat::FltP => {
            let count = (src.len() / 4).min(dst.len());
            for i in 0..count {
                dst[i] = f32::from_le_bytes(src[i * 4..i * 4 + 4].try_into().unwrap());
            }
        }
        SampleFormat::Dbl | SampleFormat::DblP => {
            let count = (src.len() / 8).min(dst.len());
            for i in 0..count {
                let d = f64::from_le_bytes(src[i * 8..i * 8 + 8].try_into().unwrap());
                dst[i] = d as f32;
            }
        }
        _ => {}
    }
}

/// Convert normalized f32 samples into raw bytes of target sample format.
pub fn f32_to_bytes(src: &[f32], fmt: SampleFormat, dst: &mut [u8]) {
    match fmt {
        SampleFormat::U8 | SampleFormat::U8P => {
            for (i, &s) in src.iter().enumerate().take(dst.len()) {
                let u = ((s * 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
                dst[i] = u;
            }
        }
        SampleFormat::S16 | SampleFormat::S16P => {
            let count = src.len().min(dst.len() / 2);
            for i in 0..count {
                let s = (src[i] * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
                dst[i * 2..i * 2 + 2].copy_from_slice(&s.to_le_bytes());
            }
        }
        SampleFormat::S32 | SampleFormat::S32P => {
            let count = src.len().min(dst.len() / 4);
            for i in 0..count {
                let s = (src[i] * 2147483647.0).round().clamp(-2147483648.0, 2147483647.0) as i32;
                dst[i * 4..i * 4 + 4].copy_from_slice(&s.to_le_bytes());
            }
        }
        SampleFormat::Flt | SampleFormat::FltP => {
            let count = src.len().min(dst.len() / 4);
            for i in 0..count {
                dst[i * 4..i * 4 + 4].copy_from_slice(&src[i].to_le_bytes());
            }
        }
        SampleFormat::Dbl | SampleFormat::DblP => {
            let count = src.len().min(dst.len() / 8);
            for i in 0..count {
                let d = src[i] as f64;
                dst[i * 8..i * 8 + 8].copy_from_slice(&d.to_le_bytes());
            }
        }
        _ => {}
    }
}
