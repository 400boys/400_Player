//! Bitstream manipulation primitives.

pub mod bitreader;
pub mod bitwriter;

pub use bitreader::{BitOrder, BitReader};
pub use bitwriter::BitWriter;
