/// 从字节切片中读取一个 u32 值并移动指针
pub fn read32(data: &mut &[u8]) -> u32 {
    let val = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    *data = &data[4..];
    val
}

/// 从字节切片中读取一个 u16 值并移动指针
pub fn read16(data: &mut &[u8]) -> u16 {
    let val = u16::from_le_bytes([data[0], data[1]]);
    *data = &data[2..];
    val
}

/// 从字节切片中读取一个 u8 值并移动指针
pub fn read8(data: &mut &[u8]) -> u8 {
    let val = data[0];
    *data = &data[1..];
    val
}

/// 获取 u16 的高位字节
pub fn _my_hibyte(v: u16) -> u8 {
    (v >> 8) as u8
}

/// 获取 u16 的低位字节
pub fn _my_lobyte(v: u16) -> u8 {
    (v & 0xFF) as u8
}

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
