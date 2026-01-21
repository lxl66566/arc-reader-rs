use std::path::Path;

use crate::{
    decrypt::{hash_update, read8, read16, read32},
    error::{ArcError, ArcResult},
    write::write_rgba_to_png,
};

/// CBG 节点结构体
#[derive(Debug, Clone)]
struct NodeCBG {
    vv: [u32; 6],
}

impl NodeCBG {
    fn new() -> Self {
        NodeCBG { vv: [0; 6] }
    }
}

/// 检查数据是否是有效的 CBG 文件
pub fn is_valid(data: &[u8], size: u32) -> bool {
    if size < 48 {
        return false;
    }

    // 检查文件魔数
    &data[0..15] == b"CompressedBG___"
}

/// 解密 CBG 文件，返回解密后的数据以及宽度和高度
pub fn decrypt(crypted: &[u8]) -> ArcResult<(Vec<u8>, u16, u16)> {
    if crypted.len() < 48 {
        return Err(ArcError::InvalidFormat);
    }
    let mut data_ptr = &crypted[16..];

    let width = read16(&mut data_ptr);
    let height = read16(&mut data_ptr);
    let bpp = read32(&mut data_ptr);

    // 跳过未使用的字段
    let _ = read32(&mut data_ptr);
    let _ = read32(&mut data_ptr);

    let intermediate_len = read32(&mut data_ptr) as usize;
    let mut key = read32(&mut data_ptr);
    let enc_len = read32(&mut data_ptr) as usize;
    let sum_check = read8(&mut data_ptr);
    let xor_check = read8(&mut data_ptr);
    let version = read16(&mut data_ptr);

    if version >= 2 && enc_len >= 0x80 {
        return Err(ArcError::UnsupportedFileType("CBG version 2".to_string()));
    }

    // 解密数据0 (Huffman 权重表)
    let mut data0 = vec![0u8; enc_len];
    let data0_src = &data_ptr[0..enc_len];
    let mut sum_data = 0u8;
    let mut xor_data = 0u8;

    for n in 0..enc_len {
        data0[n] = data0_src[n].wrapping_sub((hash_update(&mut key) & 0xFF) as u8);
        sum_data = sum_data.wrapping_add(data0[n]);
        xor_data ^= data0[n];
    }

    if sum_data != sum_check || xor_data != xor_check {
        return Err(ArcError::CbgDecryptError);
    }

    // 读取变量并建立表
    let mut ptr = &data0[..];
    let table: [u32; 256] = std::array::from_fn(|_| read_variable(&mut ptr));

    // 构建解压表
    let mut table2 = vec![NodeCBG::new(); 511];
    let root = method2(&table, &mut table2);

    // 解压数据1 (Huffman 解压)
    let mut packed_src = &data_ptr[enc_len..];
    let mut data1 = vec![0u8; intermediate_len];

    let mut mask = 0x80u8;
    let mut current_byte = 0u8;

    for n in 0..intermediate_len {
        let mut cvalue = root;
        while table2[cvalue as usize].vv[2] == 1 {
            if mask == 0x80 {
                if packed_src.is_empty() {
                    break;
                }
                current_byte = packed_src[0];
                packed_src = &packed_src[1..];
            }

            let bit = if (current_byte & mask) != 0 { 1 } else { 0 };
            mask = if mask == 0x01 { 0x80 } else { mask >> 1 };

            cvalue = table2[cvalue as usize].vv[4 + bit];
        }
        data1[n] = cvalue as u8;
    }

    // 解码数据3 (Zero 解压/RLE)
    let pixel_size = (bpp / 8) as usize;
    let stride = width as usize * pixel_size;
    let mut raw_pixels = vec![0u8; stride * height as usize];

    let mut psrc = &data1[..];
    let mut is_zero = false;
    let mut dst_idx = 0;

    while !psrc.is_empty() && dst_idx < raw_pixels.len() {
        let count = read_variable(&mut psrc) as usize;
        let end = (dst_idx + count).min(raw_pixels.len());
        if !is_zero {
            let to_copy = (end - dst_idx).min(psrc.len());
            raw_pixels[dst_idx..dst_idx + to_copy].copy_from_slice(&psrc[..to_copy]);
            psrc = &psrc[to_copy..];
        }
        dst_idx = end;
        is_zero = !is_zero;
    }

    // 逆向平均采样 (Reverse Average Sampling)
    for y in 0..height as usize {
        let line = y * stride;
        for x in 0..width as usize {
            let pixel_off = line + x * pixel_size;
            for p in 0..pixel_size {
                let mut avg = 0u32;
                if x > 0 {
                    avg += raw_pixels[pixel_off + p - pixel_size] as u32;
                }
                if y > 0 {
                    avg += raw_pixels[pixel_off + p - stride] as u32;
                }
                if x > 0 && y > 0 {
                    avg /= 2;
                }
                if avg != 0 {
                    raw_pixels[pixel_off + p] = raw_pixels[pixel_off + p].wrapping_add(avg as u8);
                }
            }
        }
    }

    // 转换为 RGBA [R, G, B, A]
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for px in 0..(width as usize * height as usize) {
        let off = px * pixel_size;
        match bpp {
            32 => {
                // BGRA -> RGBA
                pixels.extend_from_slice(&[
                    raw_pixels[off + 2],
                    raw_pixels[off + 1],
                    raw_pixels[off],
                    raw_pixels[off + 3],
                ]);
            }
            24 => {
                // BGR -> RGBA
                pixels.extend_from_slice(&[
                    raw_pixels[off + 2],
                    raw_pixels[off + 1],
                    raw_pixels[off],
                    255,
                ]);
            }
            8 => {
                // Gray -> RGBA
                let v = raw_pixels[off];
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
            16 => {
                // Bgr565 -> RGBA (Assumed B-high, R-low as per Bgr565 common usage)
                let val = u16::from_le_bytes([raw_pixels[off], raw_pixels[off + 1]]);
                let b = ((val >> 11) & 0x1F) as u8;
                let g = ((val >> 5) & 0x3F) as u8;
                let r = (val & 0x1F) as u8;
                pixels.push((r << 3) | (r >> 2));
                pixels.push((g << 2) | (g >> 4));
                pixels.push((b << 3) | (b >> 2));
                pixels.push(255);
            }
            _ => {
                pixels.extend_from_slice(&[0, 0, 0, 255]);
            }
        }
    }

    Ok((pixels, width, height))
}

/// 将 CBG 数据保存为 PNG 文件
pub fn save(data: &[u8], width: u16, height: u16, savepath: impl AsRef<Path>) -> ArcResult<()> {
    write_rgba_to_png(width, height, data, savepath.as_ref().with_extension("png"))?;
    Ok(())
}

// 辅助函数：读取可变长度整数
fn read_variable(ptr: &mut &[u8]) -> u32 {
    let mut v = 0u32;
    let mut shift = 0i32;

    while let Some(&c) = ptr.first() {
        *ptr = &ptr[1..];

        v |= ((c & 0x7F) as u32) << shift;
        shift += 7;

        if (c & 0x80) == 0 {
            return v;
        }
        if shift >= 32 {
            break;
        }
    }

    v
}

// 辅助函数：构建解压缩表
fn method2(table1: &[u32; 256], table2: &mut [NodeCBG]) -> u32 {
    let mut sum_of_values = 0u32;

    // 初始化节点
    for n in 0..256 {
        table2[n].vv[0] = if table1[n] > 0 { 1 } else { 0 };
        table2[n].vv[1] = table1[n];
        table2[n].vv[2] = 0;
        table2[n].vv[3] = !0;
        table2[n].vv[4] = n as u32;
        table2[n].vv[5] = n as u32;
        sum_of_values += table1[n];
    }

    let mut node = NodeCBG::new();
    node.vv[0] = 0;
    node.vv[1] = 0;
    node.vv[2] = 1;
    node.vv[3] = !0;
    node.vv[4] = !0;
    node.vv[5] = !0;

    for n in 0..255 {
        table2[256 + n] = node.clone();
    }

    let mut cnodes = 256;

    loop {
        let mut vinfo = [!0; 2];

        for m in 0..2 {
            let mut min_value = !0u32;

            for n in 0..cnodes {
                let cnode = &table2[n as usize];

                if cnode.vv[0] == 1 && cnode.vv[1] < min_value {
                    vinfo[m] = n;
                    min_value = cnode.vv[1];
                }
            }

            if vinfo[m] != !0 {
                table2[vinfo[m] as usize].vv[0] = 0;
                table2[vinfo[m] as usize].vv[3] = cnodes;
            }
        }

        node.vv[0] = 1;
        node.vv[1] = (if vinfo[1] != !0 {
            table2[vinfo[1] as usize].vv[1]
        } else {
            0
        }) + table2[vinfo[0] as usize].vv[1];
        node.vv[2] = 1;
        node.vv[3] = !0;
        node.vv[4] = vinfo[0];
        node.vv[5] = vinfo[1];

        table2[cnodes as usize] = node.clone();
        cnodes += 1;

        if node.vv[1] == sum_of_values {
            break;
        }
    }

    cnodes - 1
}
