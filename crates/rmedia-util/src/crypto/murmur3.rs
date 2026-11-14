//! MurmurHash3 (32-bit and 128-bit, av_murmur3 reproduction).

/// Compute 32-bit MurmurHash3.
pub fn murmur3_32(data: &[u8], seed: u32) -> u32 {
    let c1 = 0xcc9e2d51u32;
    let c2 = 0x1b873593u32;

    let mut h = seed;
    let nblocks = data.len() / 4;

    for i in 0..nblocks {
        let mut k = u32::from_le_bytes(data[i * 4..(i + 1) * 4].try_into().unwrap());
        k = k.wrapping_mul(c1);
        k = k.rotate_left(15);
        k = k.wrapping_mul(c2);

        h ^= k;
        h = h.rotate_left(13);
        h = h.wrapping_mul(5).wrapping_add(0xe6546b64);
    }

    let tail = &data[nblocks * 4..];
    let mut k1 = 0u32;
    match tail.len() {
        3 => {
            k1 ^= (tail[2] as u32) << 16;
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
            k1 = k1.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
            h ^= k1;
        }
        2 => {
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
            k1 = k1.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
            h ^= k1;
        }
        1 => {
            k1 ^= tail[0] as u32;
            k1 = k1.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
            h ^= k1;
        }
        _ => {}
    }

    h ^= data.len() as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    h
}

/// Compute 128-bit MurmurHash3 (x64 variant).
pub fn murmur3_128(data: &[u8], seed: u64) -> [u8; 16] {
    let c1 = 0x87c37b91114253d5u64;
    let c2 = 0x4cf5ad432745937fu64;

    let mut h1 = seed;
    let mut h2 = seed;

    let nblocks = data.len() / 16;
    for i in 0..nblocks {
        let chunk = &data[i * 16..(i + 1) * 16];
        let mut k1 = u64::from_le_bytes(chunk[0..8].try_into().unwrap());
        let mut k2 = u64::from_le_bytes(chunk[8..16].try_into().unwrap());

        k1 = k1.wrapping_mul(c1).rotate_left(31).wrapping_mul(c2);
        h1 ^= k1;
        h1 = h1.rotate_left(27).wrapping_add(h2).wrapping_mul(5).wrapping_add(0x52dce729);

        k2 = k2.wrapping_mul(c2).rotate_left(33).wrapping_mul(c1);
        h2 ^= k2;
        h2 = h2.rotate_left(31).wrapping_add(h1).wrapping_mul(5).wrapping_add(0x38495ab5);
    }

    let tail = &data[nblocks * 16..];
    let mut k1 = 0u64;
    let mut k2 = 0u64;

    let len = tail.len();
    if len > 8 {
        for j in (8..len).rev() {
            k2 = (k2 << 8) | (tail[j] as u64);
        }
        k2 = k2.wrapping_mul(c2).rotate_left(33).wrapping_mul(c1);
        h2 ^= k2;
    }
    if len > 0 {
        let limit = len.min(8);
        for j in (0..limit).rev() {
            k1 = (k1 << 8) | (tail[j] as u64);
        }
        k1 = k1.wrapping_mul(c1).rotate_left(31).wrapping_mul(c2);
        h1 ^= k1;
    }

    h1 ^= data.len() as u64;
    h2 ^= data.len() as u64;

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    // fmix64
    let fmix = |mut k: u64| {
        k ^= k >> 33;
        k = k.wrapping_mul(0xff51afd7ed558ccd);
        k ^= k >> 33;
        k = k.wrapping_mul(0xc4ceb9fe1a85ec53);
        k ^ (k >> 33)
    };

    h1 = fmix(h1);
    h2 = fmix(h2);

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&h1.to_le_bytes());
    out[8..16].copy_from_slice(&h2.to_le_bytes());
    out
}
