//! LZO1X lossless data compression and decompression (av_lzo reproduction).

use crate::error::{Result, RmediaError};

/// Decompress LZO1X compressed stream into output slice.
pub fn lzo1x_decompress(src: &[u8], dst: &mut [u8]) -> Result<usize> {
    let mut in_pos = 0;
    let mut out_pos = 0;

    let src_len = src.len();
    let dst_len = dst.len();

    if src_len == 0 {
        return Ok(0);
    }

    let mut state = 0usize;
    let mut x = src[in_pos];
    in_pos += 1;

    if x > 17 {
        let t = (x - 17) as usize;
        if in_pos + t > src_len || out_pos + t > dst_len {
            return Err(RmediaError::InvalidData("LZO buffer overrun".into()));
        }
        dst[out_pos..out_pos + t].copy_from_slice(&src[in_pos..in_pos + t]);
        in_pos += t;
        out_pos += t;
        state = 4;
    }

    while in_pos < src_len {
        x = src[in_pos];
        in_pos += 1;

        if x < 16 {
            if state == 0 {
                if x == 0 {
                    while in_pos < src_len && src[in_pos] == 0 {
                        x = x.saturating_add(255);
                        in_pos += 1;
                    }
                    if in_pos >= src_len {
                        return Err(RmediaError::InvalidData("LZO unexpected EOF".into()));
                    }
                    x = x.saturating_add(src[in_pos]);
                    in_pos += 1;
                }
                let t = (x + 3) as usize;
                if in_pos + t > src_len || out_pos + t > dst_len {
                    return Err(RmediaError::InvalidData("LZO buffer overrun".into()));
                }
                dst[out_pos..out_pos + t].copy_from_slice(&src[in_pos..in_pos + t]);
                in_pos += t;
                out_pos += t;
                state = 4;
                continue;
            } else if state != 4 {
                let m_dist = ((x as usize) & 0x08) << 11;
                let mut m_len = (x & 0x07) as usize;
                if in_pos >= src_len {
                    return Err(RmediaError::InvalidData("LZO unexpected EOF".into()));
                }
                let m_off = (((src[in_pos] as usize) << 6) | ((x >> 4) as usize) | m_dist) + 1;
                in_pos += 1;

                if m_len == 0 {
                    while in_pos < src_len && src[in_pos] == 0 {
                        m_len = m_len.saturating_add(255);
                        in_pos += 1;
                    }
                    if in_pos >= src_len {
                        return Err(RmediaError::InvalidData("LZO unexpected EOF".into()));
                    }
                    m_len = m_len.saturating_add(src[in_pos] as usize);
                    in_pos += 1;
                    m_len += 7;
                } else {
                    m_len += 2;
                }

                if m_off > out_pos || out_pos + m_len > dst_len {
                    return Err(RmediaError::InvalidData("LZO match offset overrun".into()));
                }

                for _ in 0..m_len {
                    dst[out_pos] = dst[out_pos - m_off];
                    out_pos += 1;
                }
                state = 0;
                continue;
            } else {
                let m_off = (x as usize) + 1;
                if in_pos >= src_len {
                    return Err(RmediaError::InvalidData("LZO unexpected EOF".into()));
                }
                let byte2 = src[in_pos] as usize;
                in_pos += 1;
                let m_off = (m_off | (byte2 << 4)) + 1;

                if m_off > out_pos || out_pos + 2 > dst_len {
                    return Err(RmediaError::InvalidData("LZO match offset overrun".into()));
                }

                dst[out_pos] = dst[out_pos - m_off];
                out_pos += 1;
                dst[out_pos] = dst[out_pos - m_off];
                out_pos += 1;
                state = 0;
                continue;
            }
        }

        if x >= 64 {
            let m_len = ((x >> 5) - 1) as usize;
            if in_pos >= src_len {
                return Err(RmediaError::InvalidData("LZO unexpected EOF".into()));
            }
            let m_off = ((((x as usize) & 0x18) << 5)
                | (src[in_pos] as usize)
                | ((((x as usize) >> 2) & 1) << 10))
                + 1;
            in_pos += 1;

            if m_off > out_pos || out_pos + m_len + 2 > dst_len {
                return Err(RmediaError::InvalidData("LZO match offset overrun".into()));
            }

            for _ in 0..(m_len + 2) {
                dst[out_pos] = dst[out_pos - m_off];
                out_pos += 1;
            }
            state = (x & 3) as usize;
            if state != 0 {
                if in_pos + state > src_len || out_pos + state > dst_len {
                    return Err(RmediaError::InvalidData("LZO buffer overrun".into()));
                }
                dst[out_pos..out_pos + state].copy_from_slice(&src[in_pos..in_pos + state]);
                in_pos += state;
                out_pos += state;
            }
        } else if x >= 32 {
            let mut m_len = (x & 31) as usize;
            if m_len == 0 {
                while in_pos < src_len && src[in_pos] == 0 {
                    m_len = m_len.saturating_add(255);
                    in_pos += 1;
                }
                if in_pos >= src_len {
                    return Err(RmediaError::InvalidData("LZO unexpected EOF".into()));
                }
                m_len = m_len.saturating_add(src[in_pos] as usize);
                in_pos += 1;
                m_len += 31;
            }
            m_len += 2;

            if in_pos + 1 >= src_len {
                return Err(RmediaError::InvalidData("LZO unexpected EOF".into()));
            }
            let m_off = ((src[in_pos] as usize) | ((src[in_pos + 1] as usize) << 8)) + 1;
            in_pos += 2;

            if m_off > out_pos || out_pos + m_len > dst_len {
                return Err(RmediaError::InvalidData("LZO match offset overrun".into()));
            }

            for _ in 0..m_len {
                dst[out_pos] = dst[out_pos - m_off];
                out_pos += 1;
            }
            state = 0;
        } else {
            // 16..31
            let m_dist = ((x as usize) & 8) << 11;
            let mut m_len = (x & 7) as usize;
            if in_pos + 1 >= src_len {
                return Err(RmediaError::InvalidData("LZO unexpected EOF".into()));
            }
            let m_off = ((src[in_pos] as usize) | ((src[in_pos + 1] as usize) << 8) | m_dist) + 1;
            in_pos += 2;

            if m_len == 0 {
                while in_pos < src_len && src[in_pos] == 0 {
                    m_len = m_len.saturating_add(255);
                    in_pos += 1;
                }
                if in_pos >= src_len {
                    return Err(RmediaError::InvalidData("LZO unexpected EOF".into()));
                }
                m_len = m_len.saturating_add(src[in_pos] as usize);
                in_pos += 1;
                m_len += 7;
            }
            m_len += 2;

            if m_off > out_pos || out_pos + m_len > dst_len {
                return Err(RmediaError::InvalidData("LZO match offset overrun".into()));
            }

            for _ in 0..m_len {
                dst[out_pos] = dst[out_pos - m_off];
                out_pos += 1;
            }
            state = 0;
        }
    }

    Ok(out_pos)
}

/// Compress slice using LZO1X-1 algorithm.
pub fn lzo1x_1_compress(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + src.len() / 16 + 64);
    if src.is_empty() {
        return out;
    }

    // Output literals chunk
    let mut in_pos = 0;
    while in_pos < src.len() {
        let chunk_len = (src.len() - in_pos).min(1024);
        if in_pos == 0 {
            if chunk_len <= 17 {
                out.push((chunk_len + 17) as u8);
            } else {
                out.push(0);
                let mut rem = chunk_len - 18;
                while rem >= 255 {
                    out.push(0);
                    rem -= 255;
                }
                out.push(rem as u8);
            }
        } else {
            out.push(0);
            let mut rem = chunk_len - 3;
            while rem >= 255 {
                out.push(0);
                rem -= 255;
            }
            out.push(rem as u8);
        }
        out.extend_from_slice(&src[in_pos..in_pos + chunk_len]);
        in_pos += chunk_len;
    }

    // End of stream marker
    out.push(17);
    out.push(0);
    out.push(0);

    out
}
