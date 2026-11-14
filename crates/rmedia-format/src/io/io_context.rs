//! Byte I/O stream context with endian-aware primitives (AVIOContext reproduction).

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use rmedia_util::error::{Result, RmediaError};

/// Internal stream backend enum.
pub enum StreamBackend {
    Cursor(Cursor<Vec<u8>>),
    File(File),
    Custom(Box<dyn CustomStream>),
}

/// Helper trait combining Read, Write, Seek, Send, Sync.
pub trait CustomStream: Read + Write + Seek + Send + Sync {}
impl<T: Read + Write + Seek + Send + Sync> CustomStream for T {}

/// Generic byte I/O stream wrapper (AVIOContext).
pub struct IoContext {
    backend: StreamBackend,
    pos: u64,
}

impl IoContext {
    /// Create an IoContext from a local file path for reading.
    pub fn open_file_read(path: &str) -> Result<Self> {
        let f = File::open(path)?;
        Ok(Self {
            backend: StreamBackend::File(f),
            pos: 0,
        })
    }

    /// Create an IoContext from a local file path for writing.
    pub fn open_file_write(path: &str) -> Result<Self> {
        let f = File::create(path)?;
        Ok(Self {
            backend: StreamBackend::File(f),
            pos: 0,
        })
    }

    /// Create an IoContext from an in-memory byte vector.
    pub fn from_vec(vec: Vec<u8>) -> Self {
        Self {
            backend: StreamBackend::Cursor(Cursor::new(vec)),
            pos: 0,
        }
    }

    /// Create an empty in-memory writer.
    pub fn new_memory() -> Self {
        Self::from_vec(Vec::new())
    }

    /// Consume the context and return the underlying byte vector if memory-backed.
    pub fn into_inner_vec(self) -> Option<Vec<u8>> {
        match self.backend {
            StreamBackend::Cursor(c) => Some(c.into_inner()),
            _ => None,
        }
    }

    /// Read an exact number of bytes into a buffer.
    pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        match &mut self.backend {
            StreamBackend::Cursor(c) => c.read_exact(buf)?,
            StreamBackend::File(f) => f.read_exact(buf)?,
            StreamBackend::Custom(s) => s.read_exact(buf)?,
        }
        self.pos += buf.len() as u64;
        Ok(())
    }

    /// Read up to `buf.len()` bytes, returning number of bytes read.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let n = match &mut self.backend {
            StreamBackend::Cursor(c) => c.read(buf)?,
            StreamBackend::File(f) => f.read(buf)?,
            StreamBackend::Custom(s) => s.read(buf)?,
        };
        self.pos += n as u64;
        Ok(n)
    }

    /// Read a single byte.
    pub fn read_u8(&mut self) -> Result<u8> {
        let mut b = [0u8; 1];
        self.read_exact(&mut b)?;
        Ok(b[0])
    }

    /// Read big-endian 16-bit integer (avio_rb16).
    pub fn read_u16_be(&mut self) -> Result<u16> {
        let mut b = [0u8; 2];
        self.read_exact(&mut b)?;
        Ok(u16::from_be_bytes(b))
    }

    /// Read little-endian 16-bit integer (avio_rl16).
    pub fn read_u16_le(&mut self) -> Result<u16> {
        let mut b = [0u8; 2];
        self.read_exact(&mut b)?;
        Ok(u16::from_le_bytes(b))
    }

    /// Read big-endian 24-bit integer (avio_rb24).
    pub fn read_u24_be(&mut self) -> Result<u32> {
        let mut b = [0u8; 3];
        self.read_exact(&mut b)?;
        Ok(((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32))
    }

    /// Read big-endian 32-bit integer (avio_rb32).
    pub fn read_u32_be(&mut self) -> Result<u32> {
        let mut b = [0u8; 4];
        self.read_exact(&mut b)?;
        Ok(u32::from_be_bytes(b))
    }

    /// Read little-endian 32-bit integer (avio_rl32).
    pub fn read_u32_le(&mut self) -> Result<u32> {
        let mut b = [0u8; 4];
        self.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    /// Read big-endian 64-bit integer (avio_rb64).
    pub fn read_u64_be(&mut self) -> Result<u64> {
        let mut b = [0u8; 8];
        self.read_exact(&mut b)?;
        Ok(u64::from_be_bytes(b))
    }

    /// Read little-endian 64-bit integer (avio_rl64).
    pub fn read_u64_le(&mut self) -> Result<u64> {
        let mut b = [0u8; 8];
        self.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    /// Write exact buffer.
    pub fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        match &mut self.backend {
            StreamBackend::Cursor(c) => c.write_all(buf)?,
            StreamBackend::File(f) => f.write_all(buf)?,
            StreamBackend::Custom(s) => s.write_all(buf)?,
        }
        self.pos += buf.len() as u64;
        Ok(())
    }

    /// Write a single byte.
    pub fn write_u8(&mut self, val: u8) -> Result<()> {
        self.write_all(&[val])
    }

    /// Write big-endian 16-bit integer (avio_wb16).
    pub fn write_u16_be(&mut self, val: u16) -> Result<()> {
        self.write_all(&val.to_be_bytes())
    }

    /// Write little-endian 16-bit integer (avio_wl16).
    pub fn write_u16_le(&mut self, val: u16) -> Result<()> {
        self.write_all(&val.to_le_bytes())
    }

    /// Write big-endian 24-bit integer (avio_wb24).
    pub fn write_u24_be(&mut self, val: u32) -> Result<()> {
        self.write_all(&[(val >> 16) as u8, (val >> 8) as u8, val as u8])
    }

    /// Write big-endian 32-bit integer (avio_wb32).
    pub fn write_u32_be(&mut self, val: u32) -> Result<()> {
        self.write_all(&val.to_be_bytes())
    }

    /// Write little-endian 32-bit integer (avio_wl32).
    pub fn write_u32_le(&mut self, val: u32) -> Result<()> {
        self.write_all(&val.to_le_bytes())
    }

    /// Write big-endian 64-bit integer (avio_wb64).
    pub fn write_u64_be(&mut self, val: u64) -> Result<()> {
        self.write_all(&val.to_be_bytes())
    }

    /// Write little-endian 64-bit integer (avio_wl64).
    pub fn write_u64_le(&mut self, val: u64) -> Result<()> {
        self.write_all(&val.to_le_bytes())
    }

    /// Current byte position in stream (avio_tell).
    pub fn tell(&self) -> u64 {
        self.pos
    }

    /// Seek to byte position in stream (avio_seek).
    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        let new_pos = match &mut self.backend {
            StreamBackend::Cursor(c) => c.seek(pos)?,
            StreamBackend::File(f) => f.seek(pos)?,
            StreamBackend::Custom(s) => s.seek(pos)?,
        };
        self.pos = new_pos;
        Ok(new_pos)
    }

    /// Skip `n` bytes forward.
    pub fn skip(&mut self, n: i64) -> Result<()> {
        self.seek(SeekFrom::Current(n))?;
        Ok(())
    }

    /// Flush output buffers.
    pub fn flush(&mut self) -> Result<()> {
        match &mut self.backend {
            StreamBackend::Cursor(c) => c.flush()?,
            StreamBackend::File(f) => f.flush()?,
            StreamBackend::Custom(s) => s.flush()?,
        }
        Ok(())
    }
}
