use std::io::{Cursor, Read};
use std::path::Path;

use crate::{
    decrypt::{hash_update, read8, read16, read32},
    error::{ArcError, ArcResult},
    write::write_rgba_to_png,
};

/// CBG 节点结构体
#[derive(Debug, Clone, Default)]
struct HuffmanNode {
    valid: bool,
    is_parent: bool,
    weight: u32,
    left_child_index: i32,
    right_child_index: i32,
}

struct HuffmanTree {
    nodes: Vec<HuffmanNode>,
}

impl HuffmanTree {
    fn new(leaf_nodes_weight: &[u32], v2: bool) -> Self {
        let mut node_list = Vec::with_capacity(leaf_nodes_weight.len() * 2);
        let mut root_node_weight = 0;

        for &weight in leaf_nodes_weight {
            let node = HuffmanNode {
                valid: weight != 0,
                weight,
                is_parent: false,
                left_child_index: 0,
                right_child_index: 0,
            };
            root_node_weight += node.weight;
            node_list.push(node);
        }

        loop {
            let mut weight = 0;
            let mut child_node_index = [-1i32; 2];

            for i in 0..2 {
                let mut min_weight = u32::MAX;
                child_node_index[i] = -1;
                let mut n = 0;

                if v2 {
                    while n < node_list.len() {
                        if node_list[n].valid {
                            min_weight = node_list[n].weight;
                            child_node_index[i] = n as i32;
                            n += 1;
                            break;
                        }
                        n += 1;
                    }
                    n = std::cmp::max(n, (i + 1) as usize);
                }

                while n < node_list.len() {
                    if node_list[n].valid && node_list[n].weight < min_weight {
                        min_weight = node_list[n].weight;
                        child_node_index[i] = n as i32;
                    }
                    n += 1;
                }

                if child_node_index[i] == -1 {
                    continue;
                }

                let idx = child_node_index[i] as usize;
                node_list[idx].valid = false;
                weight += node_list[idx].weight;
            }

            let parent_node = HuffmanNode {
                valid: true,
                is_parent: true,
                left_child_index: child_node_index[0],
                right_child_index: child_node_index[1],
                weight,
            };

            node_list.push(parent_node);
            if weight >= root_node_weight {
                break;
            }
        }

        HuffmanTree { nodes: node_list }
    }

    fn decode_token(&self, reader: &mut MsbBitStream) -> ArcResult<i32> {
        let mut node_index = (self.nodes.len() - 1) as i32;
        loop {
            let node = &self.nodes[node_index as usize];
            if !node.is_parent {
                break;
            }

            let bit = reader.get_next_bit()?;
            if bit == 0 {
                node_index = node.left_child_index;
            } else {
                node_index = node.right_child_index;
            }
        }
        Ok(node_index)
    }
}

struct MsbBitStream<'a> {
    data: &'a [u8],
    pos: usize,
    cache: u32,
    cache_size: i32,
}

impl<'a> MsbBitStream<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            cache: 0,
            cache_size: 0,
        }
    }

    fn get_next_bit(&mut self) -> ArcResult<i32> {
        if self.cache_size == 0 {
            if self.pos >= self.data.len() {
                return Err(ArcError::InvalidFormat); // EndOfStream
            }
            self.cache = self.data[self.pos] as u32;
            self.pos += 1;
            self.cache_size = 8;
        }
        self.cache_size -= 1;
        Ok(((self.cache >> self.cache_size) & 1) as i32)
    }

    fn get_bits(&mut self, count: i32) -> ArcResult<i32> {
        let mut v = 0;
        let mut remaining = count;

        while remaining > 0 {
            if self.cache_size == 0 {
                if self.pos >= self.data.len() {
                    return Ok(v << remaining); // Partial read or 0? C# doesn't strictly check here in loops sometimes, but DecodeToken does.
                }
                self.cache = self.data[self.pos] as u32;
                self.pos += 1;
                self.cache_size = 8;
            }

            let take = std::cmp::min(remaining, self.cache_size);
            let bits = (self.cache >> (self.cache_size - take)) & ((1 << take) - 1);
            v = (v << take) | (bits as i32);
            self.cache_size -= take;
            remaining -= take;
        }
        Ok(v as i32)
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

    let mut width = read16(&mut data_ptr);
    let mut height = read16(&mut data_ptr);
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

    // 解密数据0 (Huffman 权重表 或 DCT数据)
    let mut data0 = vec![0u8; enc_len];
    if enc_len > 0 {
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
    }

    // 移动指针跳过已解密的块
    data_ptr = &data_ptr[enc_len..];

    if version < 2 {
        decrypt_v1(&data0, data_ptr, width, height, bpp, intermediate_len)
    } else {
        if enc_len < 0x80 {
            return Err(ArcError::InvalidFormat);
        }
        let (pixels, w, h) = decrypt_v2(&data0, data_ptr, width, height, bpp)?;
        // Update width/height to aligned values
        width = w;
        height = h;
        Ok((pixels, width, height))
    }
}

/// 将 CBG 数据保存为 PNG 文件
pub fn save(data: &[u8], width: u16, height: u16, savepath: impl AsRef<Path>) -> ArcResult<()> {
    write_rgba_to_png(width, height, data, savepath.as_ref().with_extension("png"))?;
    Ok(())
}

fn decrypt_v1(
    weights_data: &[u8],
    packed_src: &[u8],
    width: u16,
    height: u16,
    bpp: u32,
    intermediate_len: usize,
) -> ArcResult<(Vec<u8>, u16, u16)> {
    // 读取变量并建立表
    let mut ptr = weights_data;
    let mut weights = Vec::new();
    // V1 reads weights until end of data0 (enc_len) or implicit assumption?
    // C# ReadWeightTable(enc, 0x100) reads 256 integers.
    for _ in 0..256 {
        weights.push(read_variable(&mut ptr));
    }

    // 构建解压表
    let tree = HuffmanTree::new(&weights, false);

    // 解压数据1 (Huffman 解压)
    let mut data1 = vec![0u8; intermediate_len];
    let mut reader = MsbBitStream::new(packed_src);

    for n in 0..intermediate_len {
        let token = tree.decode_token(&mut reader)?;
        data1[n] = token as u8;
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
        } else {
            // For zero blocks, memory is already 0 initialized.
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

    convert_to_rgba(&raw_pixels, width, height, bpp)
}

fn decrypt_v2(
    dct_data: &[u8],
    input_data: &[u8],
    orig_width: u16,
    orig_height: u16,
    bpp: u32,
) -> ArcResult<(Vec<u8>, u16, u16)> {
    let mut cursor = Cursor::new(input_data);

    // Read Weight Tables
    let weights1 = read_weight_table_v2(&mut cursor, 0x10)?;
    let tree1 = HuffmanTree::new(&weights1, true);

    let weights2 = read_weight_table_v2(&mut cursor, 0xB0)?;
    let tree2 = HuffmanTree::new(&weights2, true);

    let aligned_width = ((orig_width as usize + 7) & !7) as u16;
    let aligned_height = ((orig_height as usize + 7) & !7) as u16;

    let y_blocks = aligned_height as usize / 8;
    let mut offsets = vec![0i32; y_blocks + 1];

    // Keep track of input base for offset calculation
    // `input_base` in C# is (Input.Position + offsets.Length*4 - base_offset)
    // base_offset was Input.Position BEFORE reading weights.
    // Wait, C# logic:
    // base_offset = Input.Position (start of input_data)
    // Read weights... Input.Position moves.
    // offsets.Length = y_blocks + 1
    // input_base = (current_pos + offsets_len * 4 - base_offset)
    // offsets[i] = ReadInt32() - input_base
    // decoder.Input = ReadBytes(rest)
    //
    // So offsets are relative to the start of "data block" inside decoder.Input.
    // The "data block" starts after the offsets table.

    let base_pos = 0; // Relative to input_data start
    let current_pos = cursor.position() as usize;
    let input_base = (current_pos + offsets.len() * 4 - base_pos) as i32;

    for i in 0..offsets.len() {
        let val = read32_from_cursor(&mut cursor)?;
        offsets[i] = (val as i32) - input_base;
    }

    let mut decoder_input = Vec::new();
    cursor.read_to_end(&mut decoder_input)?;

    // Initialize DCT
    let dct_table = [
        1.00000000f32,
        1.38703990f32,
        1.30656302f32,
        1.17587554f32,
        1.00000000f32,
        0.78569496f32,
        0.54119611f32,
        0.27589938f32,
        1.38703990f32,
        1.92387950f32,
        1.81225491f32,
        1.63098633f32,
        1.38703990f32,
        1.08979023f32,
        0.75066054f32,
        0.38268343f32,
        1.30656302f32,
        1.81225491f32,
        1.70710683f32,
        1.53635550f32,
        1.30656302f32,
        1.02655995f32,
        0.70710677f32,
        0.36047992f32,
        1.17587554f32,
        1.63098633f32,
        1.53635550f32,
        1.38268340f32,
        1.17587554f32,
        0.92387950f32,
        0.63637930f32,
        0.32442334f32,
        1.00000000f32,
        1.38703990f32,
        1.30656302f32,
        1.17587554f32,
        1.00000000f32,
        0.78569496f32,
        0.54119611f32,
        0.27589938f32,
        0.78569496f32,
        1.08979023f32,
        1.02655995f32,
        0.92387950f32,
        0.78569496f32,
        0.61731654f32,
        0.42521504f32,
        0.21677275f32,
        0.54119611f32,
        0.75066054f32,
        0.70710677f32,
        0.63637930f32,
        0.54119611f32,
        0.42521504f32,
        0.29289323f32,
        0.14931567f32,
        0.27589938f32,
        0.38268343f32,
        0.36047992f32,
        0.32442334f32,
        0.27589938f32,
        0.21677275f32,
        0.14931567f32,
        0.07612047f32,
    ];
    let mut dct = [0.0f32; 128]; // 2 * 64
    for i in 0..0x80 {
        dct[(i >> 6) * 64 + (i & 0x3F)] = (dct_data[i] as f32) * dct_table[i & 0x3F];
    }

    let mut output = vec![0u8; (aligned_width as usize) * (aligned_height as usize) * 4];
    let pad_skip = ((aligned_width as usize >> 3) + 7) >> 3;
    let mut dst = 0;

    for i in 0..y_blocks {
        let block_offset = offsets[i] + pad_skip as i32;
        let next_offset = if i + 1 == y_blocks {
            decoder_input.len() as i32
        } else {
            offsets[i + 1]
        };

        let length = (next_offset - block_offset) as usize;
        if block_offset < 0 || (block_offset as usize) >= decoder_input.len() {
            // Handle error or just skip?
            dst += aligned_width as usize * 32; // 8 lines * 4 bytes/px
            continue;
        }

        unpack_block(
            &decoder_input[block_offset as usize
                ..std::cmp::min(block_offset as usize + length, decoder_input.len())], // Slice safely
            length, // Pass intended length to limit reading
            &mut output,
            dst,
            aligned_width as usize,
            bpp,
            &tree1,
            &tree2,
            &dct,
        )?;

        dst += aligned_width as usize * 32; // 8 rows * width * 4 bytes
    }

    let mut has_alpha = false;
    if bpp == 32 {
        has_alpha = unpack_alpha(
            &decoder_input,
            offsets[y_blocks],
            &mut output,
            aligned_width as usize,
        )?;
    }

    // Convert output (BGRA/BGR) to RGBA if needed
    // The output from UnpackBlock is BGRA or BGR in 4 bytes?
    // C# DecodeRGB outputs: B, G, R. 4th byte is untouched?
    // C# DecodeGrayscale outputs: Y, Y, Y.
    // C# UnpackAlpha touches 4th byte?
    // Format = decoder.HasAlpha ? PixelFormats.Bgra32 : PixelFormats.Bgr32;
    // PixelFormats.Bgra32 is B, G, R, A.
    // Rust usually wants RGBA for PNG.

    // Swap R and B
    for i in 0..output.len() / 4 {
        let b = output[i * 4];
        let r = output[i * 4 + 2];
        output[i * 4] = r;
        output[i * 4 + 2] = b;
        if !has_alpha {
            output[i * 4 + 3] = 255;
        }
    }

    Ok((output, aligned_width, aligned_height))
}

fn unpack_block(
    input: &[u8],
    limit: usize,
    output: &mut [u8],
    dst_offset: usize,
    width: usize,
    bpp: u32,
    tree1: &HuffmanTree,
    tree2: &HuffmanTree,
    dct: &[f32],
) -> ArcResult<()> {
    // In C# unpack_block creates new stream, read_integer reads from it
    let mut ptr = input;
    let block_size = read_variable(&mut ptr) as i32;

    // Create reader after reading integer
    let header_len = input.len() - ptr.len();
    let mut reader = MsbBitStream::new(&input[header_len..limit.min(input.len())]);

    if block_size == -1 {
        return Ok(());
    }

    let mut color_data = vec![0i16; block_size as usize];
    let mut acc = 0i32;

    let mut i = 0;
    while i < block_size {
        let count = tree1.decode_token(&mut reader)?;
        if count != 0 {
            let mut v = reader.get_bits(count)?;
            if (v >> (count - 1)) == 0 {
                v = (-1 << count | v) + 1;
            }
            acc += v;
        }
        color_data[i as usize] = acc as i16;
        i += 64;
    }

    if (reader.cache_size & 7) != 0 {
        let _ = reader.get_bits(reader.cache_size & 7)?;
    }

    let block_fill_order: [usize; 64] = [
        0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27,
        20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
        58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
    ];

    let mut i = 0;
    while i < block_size {
        let mut index = 1;
        while index < 64 {
            let code = tree2.decode_token(&mut reader)?;
            if code == 0 {
                break;
            }
            if code == 0xF {
                index += 0x10;
                continue;
            }
            index += (code & 0xF) as usize;
            if index >= 64 {
                break;
            }

            let bits = code >> 4;
            let mut v = reader.get_bits(bits)?;
            if bits != 0 && (v >> (bits - 1)) == 0 {
                v = (-1 << bits | v) + 1;
            }
            color_data[(i as usize) + block_fill_order[index]] = v as i16;
            index += 1;
        }
        i += 64;
    }

    if bpp == 8 {
        decode_grayscale(&color_data, output, dst_offset, width, dct);
    } else {
        decode_rgb(&color_data, output, dst_offset, width, dct);
    }

    Ok(())
}

fn decode_rgb(data: &[i16], output: &mut [u8], dst_offset: usize, width: usize, dct: &[f32]) {
    let block_count = width / 8;
    let mut dst = dst_offset;
    for i in 0..block_count {
        let mut src = i * 64;
        let mut ycbcr_block = [0.0f32; 192]; // 64 * 3

        for channel in 0..3 {
            decode_dct(channel, data, src, &mut ycbcr_block, dct);
            src += width * 8;
        }

        for j in 0..64 {
            let cy = ycbcr_block[j * 3];
            let cb = ycbcr_block[j * 3 + 1];
            let cr = ycbcr_block[j * 3 + 2];

            let r = cy + 1.402 * cr - 178.956;
            let g = cy - 0.34414 * cb - 0.71414 * cr + 135.95984;
            let b = cy + 1.772 * cb - 226.316;

            let y = j >> 3;
            let x = j & 7;
            let p = dst + (y * width + x) * 4;

            if p < output.len() {
                output[p] = float_to_byte(b);
                output[p + 1] = float_to_byte(g);
                output[p + 2] = float_to_byte(r);
            }
        }
        dst += 32; // 8 * 4 bytes horizontal block step? No.
        // C# dst += 32.
        // dst points to start of block in output.
        // Block is 8x8.
        // Loop 'j' fills 8x8 block.
        // The outer loop in C# iterates block_count (width/8).
        // dst += 32 means moving 8 pixels horizontally? 8 * 4 = 32. Yes.
    }
}

fn decode_grayscale(data: &[i16], output: &mut [u8], dst_offset: usize, width: usize, dct: &[f32]) {
    let block_count = width / 8;
    let mut dst = dst_offset;
    let mut src = 0;

    for _ in 0..block_count {
        let mut ycbcr_block = [0.0f32; 192];
        decode_dct(0, data, src, &mut ycbcr_block, dct);
        src += 64;

        for j in 0..64 {
            let y = j >> 3;
            let x = j & 7;
            let p = dst + (y * width + x) * 4;
            let val = ycbcr_block[j * 3] as u8; // C# casts to byte directly

            if p < output.len() {
                output[p] = val;
                output[p + 1] = val;
                output[p + 2] = val;
            }
        }
        dst += 32;
    }
}

fn decode_dct(channel: usize, data: &[i16], src: usize, ycbcr_block: &mut [f32], dct: &[f32]) {
    let d_idx = if channel > 0 { 1 } else { 0 };
    let dct_base = d_idx * 64;
    let mut tmp = [0.0f32; 64];

    // Vertical pass (rows of 8) - actually C# loops 8 times, data indices +1, +8?
    // C# code:
    // for i = 0..8
    //   check zeros at src + 8 + i, src + 16 + i ... (checking column i, spaced by 8)
    //   Wait, data is 1D array.
    //   The input data seems to be stored component-wise?
    //   Yes, src + i, src + 8 + i...
    //   This looks like column-wise processing if stride is 8?

    // C# logic translation:
    for i in 0..8 {
        // Check if AC coefficients are zero
        if data[src + 8 + i] == 0
            && data[src + 16 + i] == 0
            && data[src + 24 + i] == 0
            && data[src + 32 + i] == 0
            && data[src + 40 + i] == 0
            && data[src + 48 + i] == 0
            && data[src + 56 + i] == 0
        {
            let t = (data[src + i] as f32) * dct[dct_base + i]; // C# `DCT[d, i]` corresponds to `dct_base + i`?
            // DCT table is 2x64.
            tmp[i] = t;
            tmp[8 + i] = t;
            tmp[16 + i] = t;
            tmp[24 + i] = t;
            tmp[32 + i] = t;
            tmp[40 + i] = t;
            tmp[48 + i] = t;
            tmp[56 + i] = t;
            continue;
        }

        // Helper to access data and dct
        let g = |idx| (data[src + idx] as f32) * dct[dct_base + idx];

        let mut v1 = g(i);
        let v2 = g(8 + i);
        let mut v3 = g(16 + i);
        let mut v4 = g(24 + i);
        let mut v5 = g(32 + i);
        let mut v6 = g(40 + i);
        let mut v7 = g(48 + i);
        let mut v8 = g(56 + i);

        let mut v10 = v1 + v5;
        let mut v11 = v1 - v5;
        let v12 = v3 + v7;
        let mut v13 = (v3 - v7) * 1.414213562 - v12;
        v1 = v10 + v12;
        v7 = v10 - v12;
        v3 = v11 + v13;
        v5 = v11 - v13;
        let v14 = v2 + v8;
        let v15 = v2 - v8;
        let v16 = v6 + v4;
        let v17 = v6 - v4;
        v8 = v14 + v16;
        v11 = (v14 - v16) * 1.414213562;
        let v9 = (v17 + v15) * 1.847759065;
        v10 = 1.082392200 * v15 - v9;
        v13 = -2.613125930 * v17 + v9;
        v6 = v13 - v8;
        v4 = v11 - v6;
        let v2_res = v10 + v4;

        tmp[i] = v1 + v8;
        tmp[8 + i] = v3 + v6;
        tmp[16 + i] = v5 + v4;
        tmp[24 + i] = v7 - v2_res;
        tmp[32 + i] = v7 + v2_res;
        tmp[40 + i] = v5 - v4;
        tmp[48 + i] = v3 - v6;
        tmp[56 + i] = v1 - v8;
    }

    // Horizontal pass (rows)
    let mut dst = 0;
    for i in (0..64).step_by(8) {
        // i = 0, 8, 16 ...
        let v10 = tmp[i] + tmp[i + 4];
        let v11 = tmp[i] - tmp[i + 4];
        let v12 = tmp[i + 2] + tmp[i + 6];
        let mut v13 = tmp[i + 2] - tmp[i + 6];
        let v14 = tmp[i + 1] + tmp[i + 7];
        let v15 = tmp[i + 1] - tmp[i + 7];
        let v16 = tmp[i + 5] + tmp[i + 3];
        let v17 = tmp[i + 5] - tmp[i + 3];

        v13 = 1.414213562 * v13 - v12;
        let v1 = v10 + v12;
        let v7 = v10 - v12;
        let v3 = v11 + v13;
        let v5 = v11 - v13;
        let v8 = v14 + v16;
        let v11_val = (v14 - v16) * 1.414213562;
        let v9 = (v17 + v15) * 1.847759065;
        let v10_val = v9 - v15 * 1.082392200;
        let v13_val = v9 - v17 * 2.613125930;
        let v6 = v13_val - v8;
        let v4 = v11_val - v6;
        let v2 = v10_val - v4;

        // ycbcr_block has stride 3 (RGB/YCrCb)
        // We write to channel
        ycbcr_block[dst * 3 + channel] = float_to_short(v1 + v8) as f32;
        ycbcr_block[(dst + 1) * 3 + channel] = float_to_short(v3 + v6) as f32;
        ycbcr_block[(dst + 2) * 3 + channel] = float_to_short(v5 + v4) as f32;
        ycbcr_block[(dst + 3) * 3 + channel] = float_to_short(v7 + v2) as f32;
        ycbcr_block[(dst + 4) * 3 + channel] = float_to_short(v7 - v2) as f32;
        ycbcr_block[(dst + 5) * 3 + channel] = float_to_short(v5 - v4) as f32;
        ycbcr_block[(dst + 6) * 3 + channel] = float_to_short(v3 - v6) as f32;
        ycbcr_block[(dst + 7) * 3 + channel] = float_to_short(v1 - v8) as f32;
        dst += 8;
    }
}

fn float_to_short(f: f32) -> i16 {
    let a = 128 + (f as i32 >> 3);
    if a <= 0 {
        return 0;
    }
    if a <= 0xFF {
        return a as i16;
    }
    if a < 0x180 {
        return 0xFF;
    }
    0
}

fn float_to_byte(f: f32) -> u8 {
    if f >= 255.0 {
        return 255;
    }
    if f <= 0.0 {
        return 0;
    }
    f as u8
}

fn unpack_alpha(input: &[u8], offset: i32, output: &mut [u8], width: usize) -> ArcResult<bool> {
    if offset < 0 {
        return Ok(false);
    }
    let start = offset as usize;
    if start >= input.len() {
        return Ok(false);
    }

    let mut reader = Cursor::new(&input[start..]);
    if read32_from_cursor(&mut reader)? != 1 {
        return Ok(false);
    }

    let mut dst = 3;
    let mut ctl = 1u32 << 1;

    while dst < output.len() {
        ctl >>= 1;
        if ctl == 1 {
            ctl = (read8_from_cursor(&mut reader)? as u32) | 0x100;
        }

        if (ctl & 1) != 0 {
            let v = read16_from_cursor(&mut reader)? as i32;
            let mut x = v & 0x3F;
            if x > 0x1F {
                x |= !0x3F;
            } // Sign extension 6-bit
            let mut y = (v >> 6) & 7;
            if y != 0 {
                y |= !0x7;
            } // Sign extension 3-bit: -8
            // Rust negative numbers logic...
            // In C#: x |= -0x40 (which is 0xFFFFFFC0).
            // Rust: if x is i32, | -64.
            // x is i32. 0x3F is 63. 0x1F is 31.
            // if x > 31 (top bit of 6-bit is 1), make it negative.
            if x > 31 {
                x = x - 64;
            }

            // y is 3-bit.
            // if y != 0? Wait.
            // C#: if (y != 0) y |= -8;
            // If y is 1..7, it becomes negative?
            // 3-bit signed? 000=0.
            // If any bit is set, it ORs with ...11111000.
            // So 0 stays 0. 1 becomes -7? No, -7 is ...11111001.
            // -8 is ...11111000.
            // if y=1 (001) -> 1 | -8 = -7.
            // This logic seems to implement sign extension ONLY if non-zero?
            // No, if y!=0, it implies negative offset?
            // Actually "y" in RLE usually refers to lines back?
            if y != 0 {
                y = y - 8;
            }

            let count = ((v >> 9) & 0x7F) + 3;

            let src = dst as isize + (x + y * width as i32) as isize * 4;
            if src < 0 || src >= dst as isize {
                return Ok(false);
            }

            for _ in 0..count {
                if dst < output.len() {
                    output[dst] = output[src as usize];
                }
                dst += 4;
            }
        } else {
            if dst < output.len() {
                output[dst] = read8_from_cursor(&mut reader)?;
            }
            dst += 4;
        }
    }

    Ok(true)
}

fn read_weight_table_v2(cursor: &mut Cursor<&[u8]>, length: usize) -> ArcResult<Vec<u32>> {
    let mut weights = Vec::with_capacity(length);
    for _ in 0..length {
        let w = read_integer(cursor);
        if w == -1 {
            return Err(ArcError::InvalidFormat);
        }
        weights.push(w as u32);
    }
    Ok(weights)
}

fn read_integer(cursor: &mut Cursor<&[u8]>) -> i32 {
    let mut v = 0;
    let mut code_length = 0;
    loop {
        let mut buf = [0u8; 1];
        if cursor.read(&mut buf).unwrap_or(0) == 0 {
            return -1;
        }
        let code = buf[0] as i32;
        if code_length >= 32 {
            return -1;
        }
        v |= (code & 0x7F) << code_length;
        code_length += 7;

        if (code & 0x80) == 0 {
            break;
        }
    }
    v
}

fn read32_from_cursor(cursor: &mut Cursor<&[u8]>) -> ArcResult<u32> {
    let mut buf = [0u8; 4];
    cursor.read_exact(&mut buf).map_err(ArcError::Io)?;
    Ok(u32::from_le_bytes(buf))
}

fn read16_from_cursor(cursor: &mut Cursor<&[u8]>) -> ArcResult<u16> {
    let mut buf = [0u8; 2];
    cursor.read_exact(&mut buf).map_err(ArcError::Io)?;
    Ok(u16::from_le_bytes(buf))
}

fn read8_from_cursor(cursor: &mut Cursor<&[u8]>) -> ArcResult<u8> {
    let mut buf = [0u8; 1];
    cursor.read_exact(&mut buf).map_err(ArcError::Io)?;
    Ok(buf[0])
}

// 辅助函数：转换为 RGBA [R, G, B, A]
fn convert_to_rgba(
    raw_pixels: &[u8],
    width: u16,
    height: u16,
    bpp: u32,
) -> ArcResult<(Vec<u8>, u16, u16)> {
    let pixel_size = (bpp / 8) as usize;
    let stride = width as usize * pixel_size;
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);

    for y in 0..height as usize {
        let line = y * stride;
        for x in 0..width as usize {
            let off = line + x * pixel_size;
            match bpp {
                32 => {
                    // BGRA -> RGBA
                    if off + 3 < raw_pixels.len() {
                        pixels.extend_from_slice(&[
                            raw_pixels[off + 2],
                            raw_pixels[off + 1],
                            raw_pixels[off],
                            raw_pixels[off + 3],
                        ]);
                    }
                }
                24 => {
                    // BGR -> RGBA
                    if off + 2 < raw_pixels.len() {
                        pixels.extend_from_slice(&[
                            raw_pixels[off + 2],
                            raw_pixels[off + 1],
                            raw_pixels[off],
                            255,
                        ]);
                    }
                }
                8 => {
                    // Gray -> RGBA
                    if off < raw_pixels.len() {
                        let v = raw_pixels[off];
                        pixels.extend_from_slice(&[v, v, v, 255]);
                    }
                }
                16 => {
                    // Bgr565 -> RGBA
                    if off + 1 < raw_pixels.len() {
                        let val = u16::from_le_bytes([raw_pixels[off], raw_pixels[off + 1]]);
                        let b = ((val >> 11) & 0x1F) as u8;
                        let g = ((val >> 5) & 0x3F) as u8;
                        let r = (val & 0x1F) as u8;
                        pixels.push((r << 3) | (r >> 2));
                        pixels.push((g << 2) | (g >> 4));
                        pixels.push((b << 3) | (b >> 2));
                        pixels.push(255);
                    }
                }
                _ => {
                    pixels.extend_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
    }
    Ok((pixels, width, height))
}

// 辅助函数：读取可变长度整数 (For V1)
fn read_variable(ptr: &mut &[u8]) -> u32 {
    // Matches existing read_variable
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
