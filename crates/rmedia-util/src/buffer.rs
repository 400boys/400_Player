//! Memory buffers and reference-counted buffer pools (AVBuffer reproduction).

use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

/// Minimum memory alignment target for SIMD operations (64 bytes).
pub const AV_INPUT_BUFFER_PADDING_SIZE: usize = 64;

/// Reference-counted, growable or fixed-capacity buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignedBuffer {
    data: Vec<u8>,
}

impl AlignedBuffer {
    /// Allocate an aligned buffer of the requested capacity, zero-initialized.
    pub fn new(capacity: usize) -> Self {
        // We allocate extra padding at the end to guarantee safe SIMD over-reads
        let total = capacity + AV_INPUT_BUFFER_PADDING_SIZE;
        let mut data = Vec::with_capacity(total);
        data.resize(capacity, 0);
        Self { data }
    }

    /// Allocate from existing byte slice.
    pub fn from_slice(slice: &[u8]) -> Self {
        let mut buf = Self::new(slice.len());
        buf.data.copy_from_slice(slice);
        buf
    }

    /// Length of the active buffer in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Direct slice view of the buffer.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Direct mutable slice view of the buffer.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Resize buffer to new length.
    pub fn resize(&mut self, new_len: usize, value: u8) {
        self.data.resize(new_len, value);
    }
}

impl Deref for AlignedBuffer {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for AlignedBuffer {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

/// Atomically reference-counted buffer reference (AVBufferRef).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferRef {
    buffer: Arc<AlignedBuffer>,
}

impl BufferRef {
    /// Create a new reference-counted buffer with the given size.
    pub fn new(size: usize) -> Self {
        Self {
            buffer: Arc::new(AlignedBuffer::new(size)),
        }
    }

    /// Create from existing buffer.
    pub fn from_buffer(buf: AlignedBuffer) -> Self {
        Self {
            buffer: Arc::new(buf),
        }
    }

    /// Create from slice.
    pub fn from_slice(slice: &[u8]) -> Self {
        Self {
            buffer: Arc::new(AlignedBuffer::from_slice(slice)),
        }
    }

    /// Check if this reference is uniquely owned and writable.
    pub fn is_writable(&self) -> bool {
        Arc::strong_count(&self.buffer) == 1
    }

    /// Make the buffer writable, cloning the underlying memory if shared (Copy-On-Write).
    pub fn make_writable(&mut self) -> &mut AlignedBuffer {
        Arc::make_mut(&mut self.buffer)
    }

    /// Number of active references to this buffer.
    pub fn ref_count(&self) -> usize {
        Arc::strong_count(&self.buffer)
    }

    /// Get inner buffer slice.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        self.buffer.as_slice()
    }

    /// Get mutable inner buffer slice (Copy-On-Write).
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.make_writable().as_mut_slice()
    }
}

impl Deref for BufferRef {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.buffer.as_slice()
    }
}

impl DerefMut for BufferRef {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.make_writable().as_mut_slice()
    }
}

/// Thread-safe reusable buffer pool (AVBufferPool).
#[derive(Debug, Default)]
pub struct BufferPool {
    pool: Mutex<Vec<AlignedBuffer>>,
    buffer_size: usize,
}

impl BufferPool {
    /// Create a new buffer pool with a specific fixed buffer size.
    pub fn new(buffer_size: usize) -> Self {
        Self {
            pool: Mutex::new(Vec::new()),
            buffer_size,
        }
    }

    /// Retrieve or allocate a buffer of the pool's configured size.
    pub fn get(&self) -> BufferRef {
        let mut guard = self.pool.lock().unwrap();
        if let Some(buf) = guard.pop() {
            BufferRef::from_buffer(buf)
        } else {
            BufferRef::new(self.buffer_size)
        }
    }

    /// Return an unused buffer back to the pool for reuse.
    pub fn recycle(&self, mut buf: AlignedBuffer) {
        if buf.len() == self.buffer_size {
            buf.as_mut_slice().fill(0);
            let mut guard = self.pool.lock().unwrap();
            guard.push(buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_cow() {
        let mut b1 = BufferRef::from_slice(b"hello world");
        assert!(b1.is_writable());
        let b2 = b1.clone();
        assert!(!b1.is_writable());
        assert_eq!(b1.ref_count(), 2);

        // Making writable causes a clone
        let writable = b1.make_writable();
        writable[0] = b'H';
        assert_eq!(&b1[..], b"Hello world");
        assert_eq!(&b2[..], b"hello world");
        assert!(b1.is_writable());
    }

    #[test]
    fn test_buffer_pool() {
        let pool = BufferPool::new(1024);
        let b1 = pool.get();
        assert_eq!(b1.len(), 1024);
    }
}
