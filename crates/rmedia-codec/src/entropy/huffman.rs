//! Canonical Huffman and Variable Length Code (VLC) decoding and encoding tables.

use crate::bitstream::bitreader::BitReader;
use crate::bitstream::bitwriter::BitWriter;
use rmedia_util::error::{Result, RmediaError};

/// Huffman / VLC code entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VlcCode {
    pub symbol: u16,
    pub bits: u8,
    pub code: u32,
}

/// Lookup table for fast VLC decoding.
#[derive(Debug, Clone)]
pub struct VlcTable {
    table: Vec<(u16, u8)>, // (symbol, length)
    max_bits: usize,
}

impl VlcTable {
    /// Build a lookup table from a list of codes.
    pub fn build(codes: &[VlcCode]) -> Result<Self> {
        let max_bits = codes.iter().map(|c| c.bits as usize).max().unwrap_or(0);
        if max_bits == 0 || max_bits > 16 {
            return Err(RmediaError::InvalidArgument("Invalid VLC max bits".into()));
        }

        let table_size = 1 << max_bits;
        let mut table = vec![(0xFFFFu16, 0u8); table_size];

        for c in codes {
            if c.bits == 0 {
                continue;
            }
            let fill_bits = max_bits - c.bits as usize;
            let base_idx = (c.code as usize) << fill_bits;
            let count = 1 << fill_bits;

            for i in 0..count {
                table[base_idx + i] = (c.symbol, c.bits);
            }
        }

        Ok(Self { table, max_bits })
    }

    /// Read next symbol from BitReader.
    #[inline]
    pub fn read_symbol(&self, r: &mut BitReader) -> Result<u16> {
        let peek = r.peek_bits(self.max_bits)?;
        let (symbol, bits) = self.table[peek as usize];
        if bits == 0 || symbol == 0xFFFF {
            return Err(RmediaError::InvalidData("Invalid Huffman code in bitstream".into()));
        }
        r.skip_bits(bits as usize)?;
        Ok(symbol)
    }
}

/// Helper to write symbol using an array of VlcCode.
pub fn write_huffman_symbol(w: &mut BitWriter, codebook: &[VlcCode], symbol: u16) -> Result<()> {
    if let Some(entry) = codebook.iter().find(|c| c.symbol == symbol) {
        w.write_bits(entry.code, entry.bits as usize);
        Ok(())
    } else {
        Err(RmediaError::InvalidArgument(format!("Symbol {} not in codebook", symbol)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::bitreader::BitOrder;

    #[test]
    fn test_vlc_table() {
        let codes = [
            VlcCode { symbol: 0, bits: 1, code: 0b0 },
            VlcCode { symbol: 1, bits: 2, code: 0b10 },
            VlcCode { symbol: 2, bits: 3, code: 0b110 },
            VlcCode { symbol: 3, bits: 3, code: 0b111 },
        ];
        let vlc = VlcTable::build(&codes).unwrap();

        let mut w = BitWriter::new(BitOrder::Msb);
        write_huffman_symbol(&mut w, &codes, 0).unwrap();
        write_huffman_symbol(&mut w, &codes, 1).unwrap();
        write_huffman_symbol(&mut w, &codes, 2).unwrap();
        write_huffman_symbol(&mut w, &codes, 3).unwrap();
        write_huffman_symbol(&mut w, &codes, 0).unwrap();
        let bytes = w.into_bytes();

        let mut r = BitReader::new(&bytes, BitOrder::Msb);
        assert_eq!(vlc.read_symbol(&mut r).unwrap(), 0);
        assert_eq!(vlc.read_symbol(&mut r).unwrap(), 1);
        assert_eq!(vlc.read_symbol(&mut r).unwrap(), 2);
        assert_eq!(vlc.read_symbol(&mut r).unwrap(), 3);
        assert_eq!(vlc.read_symbol(&mut r).unwrap(), 0);
    }
}
