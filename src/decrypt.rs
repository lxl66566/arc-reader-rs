//! BGI PRNG helpers ported from the original engine.

// Integer casts mirror the C implementation.
#![allow(clippy::cast_possible_truncation)]

/// Extract the high word of a u32.
pub(crate) fn hi_word(v: u32) -> u16 {
    (v >> 16) as u16
}

/// Extract the low word of a u32.
pub(crate) fn lo_word(v: u32) -> u16 {
    (v & 0xFFFF) as u16
}

/// Advance the BGI linear-congruential PRNG and return the next 15-bit value.
pub(crate) fn hash_update(hash_val: &mut u32) -> u32 {
    let edx = 20021_u32.wrapping_mul(u32::from(lo_word(*hash_val)));
    let eax = 20021_u32
        .wrapping_mul(u32::from(hi_word(*hash_val)))
        .wrapping_add(346_u32.wrapping_mul(*hash_val))
        .wrapping_add(u32::from(hi_word(edx)));
    *hash_val = (u32::from(lo_word(eax)) << 16)
        .wrapping_add(u32::from(lo_word(edx)))
        .wrapping_add(1);
    eax & 0x7FFF
}
