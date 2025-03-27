use crate::{
    decrypt::{read8, read16, read32},
    error::{ArcError, ArcResult},
};

/// 检查数据是否是有效的 BSE 1.0 文件
pub fn is_valid(data: &[u8], size: u32) -> bool {
    if size < 80 {
        return false;
    }

    // 检查文件魔数
    &data[0..7] == b"BSE 1.0"
}

/// 解密 BSE 文件（仅前 64 字节加密）
pub fn decrypt(data: &mut [u8]) -> ArcResult<()> {
    if data.len() < 16 {
        return Err(ArcError::BseDecryptError);
    }

    let mut _hash: i32 = 0;
    let mut _sum_check: u8 = 0;
    let mut _xor_check: u8 = 0;
    let mut sum_data: u8 = 0;
    let mut xor_data: u8 = 0;
    let mut flags = [0; 64];

    let mut data_mut = &data[8..];

    // 读取 0x100
    let _ = read16(&mut data_mut);
    _sum_check = read8(&mut data_mut);
    _xor_check = read8(&mut data_mut);
    _hash = read32(&mut data_mut) as i32;

    for _ in 0..64 {
        let mut _target = 0;
        let mut r = bse_rand(&mut _hash);
        let mut i = r & 0x3F;

        while flags[i as usize] != 0 {
            i = (i + 1) & 0x3F;
        }

        r = bse_rand(&mut _hash);
        let s = r & 0x07;
        _target = i as usize;

        let k = bse_rand(&mut _hash);
        r = bse_rand(&mut _hash);
        r = ((data[_target + 16] as i32 & 255) - r) & 255;

        if (k & 1) != 0 {
            data[_target + 16] = ((r << s) | (r >> (8 - s))) as u8;
        } else {
            data[_target + 16] = ((r >> s) | (r << (8 - s))) as u8;
        }

        flags[i as usize] = 1;
    }

    // 计算校验和
    for counter in 0..64 {
        sum_data = sum_data.wrapping_add(data[counter + 16]);
        xor_data ^= data[counter + 16];
    }

    if sum_data == _sum_check && xor_data == _xor_check {
        Ok(())
    } else {
        Err(ArcError::BseDecryptError)
    }
}

/// BSE 随机数生成器
fn bse_rand(seed: &mut i32) -> i32 {
    let tmp = ((((*seed * 257) >> 8) + *seed * 97) + 23) ^ -1496474763;
    *seed = ((tmp >> 16) & 65535) | (tmp << 16);
    *seed & 32767
}
