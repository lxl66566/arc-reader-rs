/// 获取 u32 的高位字
pub fn my_hiword(v: u32) -> u16 {
    (v >> 16) as u16
}

/// 获取 u32 的低位字
pub fn my_loword(v: u32) -> u16 {
    (v & 0xFFFF) as u16
}

/// 更新哈希值
pub fn hash_update(hash_val: &mut u32) -> u32 {
    let edx = 20021_u32.wrapping_mul(my_loword(*hash_val) as u32);
    let eax = 20021_u32
        .wrapping_mul(my_hiword(*hash_val) as u32)
        .wrapping_add(346_u32.wrapping_mul(*hash_val))
        .wrapping_add(my_hiword(edx) as u32);
    *hash_val = ((my_loword(eax) as u32) << 16)
        .wrapping_add(my_loword(edx) as u32)
        .wrapping_add(1);
    eax & 0x7FFF
}
