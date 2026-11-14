//! Compressed bitstream packet representation (AVPacket reproduction).

use crate::buffer::BufferRef;
use crate::rational::{Rational, NOPTS_VALUE};

/// Packet flags (AV_PKT_FLAG_*).
pub const AV_PKT_FLAG_KEY: u32 = 0x0001;
pub const AV_PKT_FLAG_CORRUPT: u32 = 0x0002;
pub const AV_PKT_FLAG_DISCARD: u32 = 0x0004;
pub const AV_PKT_FLAG_TRUSTED: u32 = 0x0008;

/// Packet side data types (AVPacketSideDataType).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PacketSideDataType {
    Palette,
    NewExtradata,
    ParamChange,
    Hdr10Plus,
    MasteringDisplayMetadata,
    ContentLightLevel,
    A53ClosedCaptions,
    Custom(String),
}

/// Packet side data element (AVPacketSideData).
#[derive(Debug, Clone, PartialEq)]
pub struct PacketSideData {
    pub side_type: PacketSideDataType,
    pub data: Vec<u8>,
}

/// Compressed bitstream packet (AVPacket).
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    /// Underlying packet payload data buffer.
    pub buf: Option<BufferRef>,
    /// Presentation timestamp in time_base units.
    pub pts: i64,
    /// Decompression timestamp in time_base units.
    pub dts: i64,
    /// Duration of this packet in time_base units.
    pub duration: i64,
    /// Index of the stream this packet belongs to.
    pub stream_index: i32,
    /// Packet flags combination (AV_PKT_FLAG_*).
    pub flags: u32,
    /// Byte position in stream where packet was read (-1 if unknown).
    pub pos: i64,
    /// Timebase of the timestamps.
    pub time_base: Rational,
    /// Packet side data list.
    pub side_data: Vec<PacketSideData>,
}

impl Default for Packet {
    fn default() -> Self {
        Self {
            buf: None,
            pts: NOPTS_VALUE,
            dts: NOPTS_VALUE,
            duration: 0,
            stream_index: 0,
            flags: 0,
            pos: -1,
            time_base: Rational::new(1, 1000),
            side_data: Vec::new(),
        }
    }
}

impl Packet {
    /// Create a new empty packet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a packet from an existing byte slice.
    pub fn from_slice(slice: &[u8]) -> Self {
        let mut pkt = Self::default();
        pkt.buf = Some(BufferRef::from_slice(slice));
        pkt
    }

    /// Create a packet wrapping a reference-counted buffer.
    pub fn from_buffer(buf: BufferRef) -> Self {
        let mut pkt = Self::default();
        pkt.buf = Some(buf);
        pkt
    }

    /// Return slice of the packet's payload data, or empty slice if none.
    pub fn data(&self) -> &[u8] {
        self.buf.as_deref().unwrap_or(&[])
    }

    /// Size of payload data in bytes.
    pub fn size(&self) -> usize {
        self.data().len()
    }

    /// Check if packet is empty.
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Check if this packet is a keyframe.
    pub fn is_keyframe(&self) -> bool {
        (self.flags & AV_PKT_FLAG_KEY) != 0
    }

    /// Set or clear the keyframe flag.
    pub fn set_keyframe(&mut self, is_key: bool) {
        if is_key {
            self.flags |= AV_PKT_FLAG_KEY;
        } else {
            self.flags &= !AV_PKT_FLAG_KEY;
        }
    }

    /// Add side data to packet.
    pub fn add_side_data(&mut self, side_type: PacketSideDataType, data: Vec<u8>) {
        if let Some(pos) = self.side_data.iter().position(|sd| sd.side_type == side_type) {
            self.side_data[pos] = PacketSideData { side_type, data };
        } else {
            self.side_data.push(PacketSideData { side_type, data });
        }
    }

    /// Retrieve side data by type.
    pub fn get_side_data(&self, side_type: &PacketSideDataType) -> Option<&[u8]> {
        self.side_data
            .iter()
            .find(|sd| &sd.side_type == side_type)
            .map(|sd| sd.data.as_slice())
    }

    /// Ensure the payload buffer is uniquely owned (Copy-On-Write).
    pub fn make_writable(&mut self) {
        if let Some(ref mut b) = self.buf {
            b.make_writable();
        }
    }
}
