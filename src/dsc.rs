use crate::decrypt::{hash_update, read8, read16, read32};
use crate::error::ArcResult;
use crate::write::write_rgba_to_png;
use std::fs::File;
use std::io::Write;

/// DSC 节点结构体
#[derive(Debug, Clone)]
struct NodeDSC {
    has_childs: u32,
    leaf_value: u32,
    childs: [u32; 2],
}

impl NodeDSC {
    fn new() -> Self {
        NodeDSC {
            has_childs: 0,
            leaf_value: 0,
            childs: [0, 0],
        }
    }
}

/// 检查数据是否是有效的 DSC 文件
pub fn is_valid(data: &[u8], size: u32) -> bool {
    if size < 32 {
        return false;
    }

    // 检查文件魔数
    &data[0..15] == b"DSC FORMAT 1.00"
}

/// 解密 DSC 文件，返回解密后的数据和大小
pub fn decrypt(crypted: &[u8], crypted_size: u32) -> ArcResult<(Vec<u8>, u32)> {
    let mut data_ptr = &crypted[16..];

    let mut hash = read32(&mut data_ptr);
    let size = read32(&mut data_ptr);
    let _ = read32(&mut data_ptr); // v2
    let _ = read32(&mut data_ptr); // padding

    let mut nodes = vec![NodeDSC::new(); 1024];

    // 构建缓冲区
    let mut buffer = Vec::with_capacity(512);
    for n in 0..512 {
        let v = crypted[n + 32].wrapping_sub((hash_update(&mut hash) & 0xFF) as u8);
        if v != 0 {
            buffer.push(((v as u32) << 16) + n as u32);
        }
    }

    // 对缓冲区排序
    buffer.sort();

    // 构建解压缩树
    let mut vector0 = vec![0u32; 1024];
    let mut nn = 0;
    let mut toggle = 0x200;
    let mut dec0 = 1;
    let mut value_set = 1;
    let mut v13_idx = 0;

    let mut buffer_cur = 0;
    while buffer_cur < buffer.len() {
        let mut vector0_ptr_idx = toggle;
        let vector0_ptr_init_idx = vector0_ptr_idx;
        let mut group_count = 0;

        while buffer_cur < buffer.len() && nn == ((buffer[buffer_cur] >> 16) & 0xFFFF) {
            nodes[vector0[v13_idx] as usize].has_childs = 0;
            nodes[vector0[v13_idx] as usize].leaf_value = buffer[buffer_cur] & 0x1FF;
            buffer_cur += 1;
            v13_idx += 1;
            group_count += 1;
        }

        let v18 = 2 * (dec0 - group_count);
        if group_count < dec0 {
            dec0 -= group_count;
            for _ in 0..dec0 {
                nodes[vector0[v13_idx] as usize].has_childs = 1;
                for m in 0..2 {
                    vector0[vector0_ptr_idx] = value_set;
                    nodes[vector0[v13_idx] as usize].childs[m] = value_set;
                    value_set += 1;
                    vector0_ptr_idx += 1;
                }
                v13_idx += 1;
            }
        }
        dec0 = v18;
        v13_idx = vector0_ptr_init_idx;
        toggle ^= 0x200;
        nn += 1;
    }

    // 解压缩数据
    let mut data = vec![0u8; size as usize];
    let src_ptr_start = 32 + 512;

    let src_end = crypted_size - src_ptr_start;
    let dst_end = size;

    let mut src_ptr = 0;
    let mut dst_ptr = 0;

    let mut bits = 0u32;
    let mut nbits = 0u32;

    while src_ptr < src_end && dst_ptr < dst_end {
        let mut nentry = 0;

        // 遍历树
        while nodes[nentry as usize].has_childs != 0 {
            if nbits == 0 {
                nbits = 8;
                bits = crypted[src_ptr_start as usize + src_ptr as usize] as u32;
                src_ptr += 1;
            }

            let bit = (bits >> 7) & 1;
            nentry = nodes[nentry as usize].childs[bit as usize];

            bits = (bits << 1) & 0xFF;
            nbits -= 1;
        }

        let info = nodes[nentry as usize].leaf_value as u16;

        if ((info >> 8) & 0xFF) as u8 == 1 {
            // Copy from previous data
            let mut cvalue = bits >> (8 - nbits);
            let mut nbits2 = nbits;

            if nbits < 12 {
                let bytes = ((11 - nbits) >> 3) + 1;
                let mut bytes_left = bytes;
                while bytes_left > 0 {
                    let next_byte = crypted[src_ptr_start as usize + src_ptr as usize] as u32;
                    cvalue = next_byte + (cvalue << 8);
                    src_ptr += 1;
                    nbits2 += 8;
                    bytes_left -= 1;
                }
            }

            nbits = nbits2 - 12;
            bits = (cvalue << (8 - (nbits2 - 12))) & 0xFF;

            let offset = (cvalue >> (nbits2 - 12)) + 2;
            let mut ring_ptr = dst_ptr - offset;
            let mut count = (info & 0xFF) as u32 + 2;

            while count > 0 {
                let tmp = data[ring_ptr as usize];
                data[dst_ptr as usize] = tmp;
                dst_ptr += 1;
                ring_ptr += 1;
                count -= 1;
            }
        } else {
            // Direct byte
            data[dst_ptr as usize] = (info & 0xFF) as u8;
            dst_ptr += 1;
        }
    }

    Ok((data, size))
}

/// 检查数据是否是图像
fn dsc_is_image(data: &[u8]) -> bool {
    if data.len() < 16 {
        return false;
    }

    let mut ptr = data;
    let width = read16(&mut ptr);
    if width == 0 || width > 8096 {
        return false;
    }

    let height = read16(&mut ptr);
    if height == 0 || height > 8096 {
        return false;
    }

    let bpp = read8(&mut ptr);
    if bpp != 8 && bpp != 24 && bpp != 32 {
        return false;
    }

    // 检查 11 个零字节
    for _ in 0..11 {
        if read8(&mut ptr) != 0 {
            return false;
        }
    }

    true
}

/// 保存 DSC 数据，如果是图像则保存为 PNG，否则保存为原始文件
pub fn save(data: &[u8], size: u32, filename: &str) -> ArcResult<()> {
    // 检查是否为图像
    if size > 15 && dsc_is_image(data) {
        let mut data_ptr = data;
        let width = read16(&mut data_ptr);
        let height = read16(&mut data_ptr);
        let bpp = read8(&mut data_ptr);

        // 跳过 11 个零字节
        data_ptr = &data_ptr[11..];

        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let mut pixels_ptr = 0;

        for _ in 0..height {
            for _ in 0..width {
                let mut r = 0;
                let mut g = 0;
                let mut b = 0;
                let mut a = 255;

                if bpp == 8 {
                    b = read8(&mut data_ptr);
                    g = b;
                    r = b;
                } else {
                    b = read8(&mut data_ptr);
                    g = read8(&mut data_ptr);
                    r = read8(&mut data_ptr);
                    if bpp == 32 {
                        a = read8(&mut data_ptr);
                    }
                }

                pixels[pixels_ptr] = r;
                pixels_ptr += 1;
                pixels[pixels_ptr] = g;
                pixels_ptr += 1;
                pixels[pixels_ptr] = b;
                pixels_ptr += 1;
                pixels[pixels_ptr] = a;
                pixels_ptr += 1;
            }
        }

        let file_name = format!("{}.png", filename);
        write_rgba_to_png(width, height, &pixels, &file_name)?;
    } else {
        // 保存为原始文件
        File::create(filename)?.write_all(&data[0..size as usize])?;
    }
    Ok(())
}
