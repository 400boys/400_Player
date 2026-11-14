//! Audio channel layout rematrixing and downmixing/upmixing (ITU-R BS.775 / Dolby).

use rmedia_util::channel_layout::{Channel, ChannelLayout};
use rmedia_util::error::{Result, RmediaError};

/// Channel rematrixing matrix of weights [out_ch][in_ch].
#[derive(Debug, Clone)]
pub struct ChannelMatrix {
    pub in_channels: usize,
    pub out_channels: usize,
    pub matrix: Vec<Vec<f32>>,
}

impl ChannelMatrix {
    /// Compute mixing matrix between two channel layouts according to ITU-R standards.
    pub fn build(in_layout: ChannelLayout, out_layout: ChannelLayout) -> Self {
        let in_ch = in_layout.nb_channels as usize;
        let out_ch = out_layout.nb_channels as usize;
        let mut matrix = vec![vec![0.0f32; in_ch]; out_ch];

        let inv_sqrt2 = std::f32::consts::FRAC_1_SQRT_2;

        if in_layout == out_layout {
            // Identity
            for i in 0..in_ch.min(out_ch) {
                matrix[i][i] = 1.0;
            }
        } else if in_layout == ChannelLayout::STEREO && out_layout == ChannelLayout::MONO {
            // Stereo -> Mono
            matrix[0][0] = inv_sqrt2;
            matrix[0][1] = inv_sqrt2;
        } else if in_layout == ChannelLayout::MONO && out_layout == ChannelLayout::STEREO {
            // Mono -> Stereo
            matrix[0][0] = 1.0;
            matrix[1][0] = 1.0;
        } else if (in_layout == ChannelLayout::LAYOUT_5POINT1 || in_layout == ChannelLayout::LAYOUT_5POINT1_BACK)
            && out_layout == ChannelLayout::STEREO
        {
            // 5.1 -> Stereo (ITU-R BS.775)
            // Channels in 5.1: 0:FL, 1:FR, 2:FC, 3:LFE, 4:SL, 5:SR
            matrix[0][0] = 1.0; // Left <- FL
            matrix[0][2] = inv_sqrt2; // Left <- FC * 0.7071
            matrix[0][4] = inv_sqrt2; // Left <- SL * 0.7071
            matrix[0][3] = 0.5; // Left <- LFE * 0.5

            matrix[1][1] = 1.0; // Right <- FR
            matrix[1][2] = inv_sqrt2; // Right <- FC * 0.7071
            matrix[1][5] = inv_sqrt2; // Right <- SR * 0.7071
            matrix[1][3] = 0.5; // Right <- LFE * 0.5
        } else if in_layout == ChannelLayout::LAYOUT_7POINT1 && out_layout == ChannelLayout::STEREO {
            // 7.1 -> Stereo
            // 0:FL, 1:FR, 2:FC, 3:LFE, 4:BL, 5:BR, 6:SL, 7:SR
            matrix[0][0] = 1.0;
            matrix[0][2] = inv_sqrt2;
            matrix[0][4] = inv_sqrt2 * 0.7;
            matrix[0][6] = inv_sqrt2 * 0.7;
            matrix[0][3] = 0.5;

            matrix[1][1] = 1.0;
            matrix[1][2] = inv_sqrt2;
            matrix[1][5] = inv_sqrt2 * 0.7;
            matrix[1][7] = inv_sqrt2 * 0.7;
            matrix[1][3] = 0.5;
        } else {
            // Fallback generic mapping by position
            for o in 0..out_ch {
                for i in 0..in_ch {
                    if o == i {
                        matrix[o][i] = 1.0;
                    }
                }
            }
        }

        Self {
            in_channels: in_ch,
            out_channels: out_ch,
            matrix,
        }
    }

    /// Apply rematrixing to planar float audio buffers in place or from src to dst.
    pub fn process_f32(&self, src: &[&[f32]], dst: &mut [&mut [f32]], nb_samples: usize) {
        for out_idx in 0..self.out_channels {
            let dst_ch = &mut dst[out_idx][..nb_samples];
            dst_ch.fill(0.0);

            for in_idx in 0..self.in_channels {
                let weight = self.matrix[out_idx][in_idx];
                if weight.abs() > 1e-6 {
                    let src_ch = &src[in_idx][..nb_samples];
                    for s in 0..nb_samples {
                        dst_ch[s] += src_ch[s] * weight;
                    }
                }
            }
        }
    }
}
