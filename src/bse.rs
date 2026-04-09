//! BSE (BuriKo Stream Encryption) decryption.
//!
//! Supports BSE 1.0 and BSE 1.1, matching GARBro's ArcBGI.cs implementation.

use smallvec::SmallVec;

use crate::error::{ArcError, ArcResult};

/// Check whether the data starts with a valid BSE 1.x signature.
pub fn is_valid(data: &[u8], size: u32) -> bool {
    if size < 0x50 {
        return false;
    }

    &data[0..6] == b"BSE 1."
}

/// Decrypt the BSE header in place (first 64 bytes of the payload).
///
/// The BSE header occupies bytes 0x10..0x50 of the file. Only those 64 bytes
/// are encrypted; the rest of the file (body from 0x50 onwards) is plaintext.
pub fn decrypt(data: &mut [u8]) -> ArcResult<()> {
    if data.len() < 0x50 {
        return Err(ArcError::BseDecryptError);
    }

    // Read version from offset 0x08
    let version = u16::from_le_bytes([data[8], data[9]]);
    if version != 0x100 && version != 0x101 {
        return Err(ArcError::BseDecryptError);
    }

    // Read header fields
    let _ = read16_from(data, 0x08); // version (already read above)
    let sum_check = data[0x0A];
    let xor_check = data[0x0B];
    let key = read32_from(data, 0x0C) as i32;

    // Decrypt the 0x40-byte header starting at offset 0x10
    let mut flags: SmallVec<[bool; 64]> = SmallVec::from([false; 0x40]);

    let mut hash = key;
    for _ in 0..0x40 {
        let rand1 = bse_next_key(&mut hash, version);
        let mut dst = (rand1 & 0x3F) as usize;
        while flags[dst] {
            dst = (dst + 1) & 0x3F;
        }

        let shift = (bse_next_key(&mut hash, version) & 7) as u32;
        let right_shift = (bse_next_key(&mut hash, version) & 1) == 0;
        let symbol = (data[0x10 + dst] as i32).wrapping_sub(bse_next_key(&mut hash, version));

        data[0x10 + dst] = if right_shift {
            rot_byte_r(symbol as u8, shift)
        } else {
            rot_byte_l(symbol as u8, shift)
        };

        flags[dst] = true;
    }

    // Verify checksums
    let mut sum_data: u8 = 0;
    let mut xor_data: u8 = 0;
    for i in 0..0x40 {
        sum_data = sum_data.wrapping_add(data[0x10 + i]);
        xor_data ^= data[0x10 + i];
    }

    if sum_data == sum_check && xor_data == xor_check {
        Ok(())
    } else {
        Err(ArcError::BseDecryptError)
    }
}

/// BSE key generator (dispatches to version-specific PRNG).
fn bse_next_key(seed: &mut i32, version: u16) -> i32 {
    if version == 0x101 {
        bse_rand_101(seed)
    } else {
        bse_rand_100(seed)
    }
}

/// BSE 1.0 random number generator (BseGenerator100 in GARBro).
fn bse_rand_100(seed: &mut i32) -> i32 {
    let s = *seed;
    let tmp = ((s.wrapping_mul(257) >> 8)
        .wrapping_add(s.wrapping_mul(97))
        .wrapping_add(23))
        ^ (-1496474763i32);
    *seed = tmp.rotate_left(16);
    *seed
}

/// BSE 1.1 random number generator (BseGenerator101 in GARBro).
fn bse_rand_101(seed: &mut i32) -> i32 {
    let s = *seed;
    let tmp = ((s.wrapping_mul(127) >> 7)
        .wrapping_add(s.wrapping_mul(83))
        .wrapping_add(53))
        ^ (-1187621284i32); // 0xB97A7E5C as i32
    *seed = tmp.rotate_left(16);
    *seed
}

// --- Helpers ---

fn read32_from(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn read16_from(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

/// Rotate byte right by `count` bits.
fn rot_byte_r(v: u8, count: u32) -> u8 {
    let count = count & 7;
    v.rotate_right(count)
}

/// Rotate byte left by `count` bits.
fn rot_byte_l(v: u8, count: u32) -> u8 {
    let count = count & 7;
    v.rotate_left(count)
}
