use crate::decrypt::{hash_update, read8, read16, read32};
use crate::error::{ArcError, ArcResult};
use crate::write::write_rgba_to_png;

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
    let mut data_ptr = &crypted[16..];

    let width = read16(&mut data_ptr);
    let height = read16(&mut data_ptr);
    let bpp = read32(&mut data_ptr);

    // 跳过未使用的字段
    let _ = read32(&mut data_ptr);
    let _ = read32(&mut data_ptr);

    let data1_len = read32(&mut data_ptr);
    let mut data0_val = read32(&mut data_ptr);
    let data0_len = read32(&mut data_ptr);
    let sum_check = read8(&mut data_ptr);
    let xor_check = read8(&mut data_ptr);

    // 读取未知字段
    let _ = read16(&mut data_ptr);

    // 解密数据0
    let mut data0 = vec![0u8; data0_len as usize];
    let data0_src = &data_ptr[0..data0_len as usize];
    let mut sum_data = 0u8;
    let mut xor_data = 0u8;

    for n in 0..data0_len {
        data0[n as usize] =
            data0_src[n as usize].wrapping_sub((hash_update(&mut data0_val) & 0xFF) as u8);
        sum_data = sum_data.wrapping_add(data0[n as usize]);
        xor_data ^= data0[n as usize];
    }

    if sum_data != sum_check || xor_data != xor_check {
        return Err(ArcError::CbgDecryptError);
    }

    // 读取变量并建立表
    let mut ptr = &data0[..];
    let table: [u32; 256] = std::array::from_fn(|_| read_variable(&mut ptr));

    // 执行方法2，构建解压表
    let mut table2 = vec![NodeCBG::new(); 511];
    let method2_res = method2(&table, &mut table2);

    // 解压数据1
    data_ptr = &data_ptr[data0_len as usize..];
    let mut data1 = vec![0u8; data1_len as usize];

    let mut mask = 0x80u8;
    let mut current_byte = 0u8;

    for n in 0..data1_len {
        let mut cvalue = method2_res;

        if table2[method2_res as usize].vv[2] == 1 {
            loop {
                if mask == 0x80 {
                    current_byte = data_ptr[0];
                    data_ptr = &data_ptr[1..];
                }

                let bit = if (current_byte & mask) != 0 { 1 } else { 0 };
                mask = if mask == 0x01 { 0x80 } else { mask >> 1 };

                cvalue = table2[cvalue as usize].vv[4 + bit];

                if table2[cvalue as usize].vv[2] != 1 {
                    break;
                }
            }
        }

        data1[n as usize] = cvalue as u8;
    }

    // 解码数据3
    let mut data3 = Vec::with_capacity(width as usize * height as usize * 4);
    let mut psrc = &data1[..];
    let mut type_flag = false;

    while !psrc.is_empty() {
        let len = read_variable(&mut psrc) as usize;
        if type_flag {
            data3.resize(data3.len() + len, 0);
        } else {
            data3.extend_from_slice(&psrc[..len]);
            psrc = &psrc[len..];
        }
        type_flag = !type_flag;
    }

    // 解码图像数据
    let mut data = vec![0u32; (width as usize) * (height as usize)];
    let mut src = &data3[..];

    let mut c = 0u32;

    // 第一行
    for x in 0..width {
        c = color_add(c, extract(&mut src, bpp));
        data[x as usize] = c;
    }

    // 其余行
    for y in 1..height {
        let row_start = y as usize * width as usize;
        let prev_row_start = (y - 1) as usize * width as usize;

        // 每行第一个像素
        c = color_add(data[prev_row_start], extract(&mut src, bpp));
        data[row_start] = c;

        // 每行其余像素
        for x in 1..width {
            let moy = color_avg(c, data[prev_row_start + x as usize]);
            c = color_add(moy, extract(&mut src, bpp));
            data[row_start + x as usize] = c;
        }
    }

    let pixels: Vec<u8> = (0..(width as usize * height as usize))
        .flat_map(|px| {
            let (r, g, b, a) = if bpp == 32 {
                (
                    ((data[px] >> 16) & 0xFF) as u8,
                    ((data[px] >> 8) & 0xFF) as u8,
                    (data[px] & 0xFF) as u8,
                    ((data[px] >> 24) & 0xFF) as u8,
                )
            } else {
                (
                    (data[px] & 0xFF) as u8,
                    ((data[px] >> 8) & 0xFF) as u8,
                    ((data[px] >> 16) & 0xFF) as u8,
                    0xFF,
                )
            };
            [r, g, b, a]
        })
        .collect();

    Ok((pixels, width, height))
}

/// 将 CBG 数据保存为 PNG 文件
pub fn save(data: &[u8], width: u16, height: u16, filename: &str) -> ArcResult<()> {
    let file_name = format!("{}.png", filename);
    write_rgba_to_png(width, height, data, &file_name)?;
    Ok(())
}

// 辅助函数：读取可变长度整数
fn read_variable(ptr: &mut &[u8]) -> u32 {
    let mut v = 0u32;
    let mut shift = 0i32;

    loop {
        let c = ptr[0];
        *ptr = &ptr[1..];

        v |= ((c & 0x7F) as u32) << shift;
        shift += 7;

        if (c & 0x80) == 0 {
            break;
        }
    }

    v
}

// 辅助函数：颜色平均值
fn color_avg(x: u32, y: u32) -> u32 {
    let a = (((x & 0xFF000000) / 2) + ((y & 0xFF000000) / 2)) & 0xFF000000;
    let r = (((x & 0x00FF0000) + (y & 0x00FF0000)) / 2) & 0x00FF0000;
    let g = (((x & 0x0000FF00) + (y & 0x0000FF00)) / 2) & 0x0000FF00;
    let b = (((x & 0x000000FF) + (y & 0x000000FF)) / 2) & 0x000000FF;

    a | r | g | b
}

// 辅助函数：颜色加法
fn color_add(x: u32, y: u32) -> u32 {
    let a = ((x & 0xFF000000) + (y & 0xFF000000)) & 0xFF000000;
    let r = ((x & 0x00FF0000) + (y & 0x00FF0000)) & 0x00FF0000;
    let g = ((x & 0x0000FF00) + (y & 0x0000FF00)) & 0x0000FF00;
    let b = ((x & 0x000000FF) + (y & 0x000000FF)) & 0x000000FF;

    a | r | g | b
}

// 辅助函数：提取颜色
fn extract(src: &mut &[u8], bpp: u32) -> u32 {
    if bpp == 32 {
        read32(src)
    } else {
        let r = read8(src);
        let (g, b) = if bpp == 24 {
            (read8(src), read8(src))
        } else {
            (r, r)
        };

        0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }
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
