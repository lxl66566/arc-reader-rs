/// Extract the high word of a u32.
pub(crate) fn my_hiword(v: u32) -> u16 {
    (v >> 16) as u16
}

/// Extract the low word of a u32.
pub(crate) fn my_loword(v: u32) -> u16 {
    (v & 0xFFFF) as u16
}

/// Advance the BGI linear-congruential PRNG and return the next 15-bit value.
pub(crate) fn hash_update(hash_val: &mut u32) -> u32 {
    let edx = 20021_u32.wrapping_mul(u32::from(my_loword(*hash_val)));
    let eax = 20021_u32
        .wrapping_mul(u32::from(my_hiword(*hash_val)))
        .wrapping_add(346_u32.wrapping_mul(*hash_val))
        .wrapping_add(u32::from(my_hiword(edx)));
    *hash_val = (u32::from(my_loword(eax)) << 16)
        .wrapping_add(u32::from(my_loword(edx)))
        .wrapping_add(1);
    eax & 0x7FFF
}
