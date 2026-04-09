//! CompressedBG (CBG) image format decoder.
//!
//! Supports both V1 (Huffman + delta prediction) and V2 (DCT + Huffman)
//! decoding, ported from GARBro's ImageCBG.cs implementation.

use std::{iter, path::Path};

use bytes::Buf;
use rayon::prelude::*;

use crate::{
    decrypt::hash_update,
    error::{ArcError, ArcResult},
    write::write_rgba_to_png,
};

// Type aliases for better readability
type DctCoefficients = [[f32; 64]; 2];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check whether `data` starts with the CompressedBG magic signature.
pub fn is_valid(data: &[u8], size: u32) -> bool {
    size >= 0x30 && data.len() >= 15 && &data[0..15] == b"CompressedBG___"
}

/// Decrypt a CompressedBG buffer, returning (RGBA pixels, width, height).
pub fn decrypt(crypted: &[u8]) -> ArcResult<(Vec<u8>, u16, u16)> {
    let mut ptr = &crypted[16..];

    let width = ptr.get_u16_le();
    let height = ptr.get_u16_le();
    let bpp = ptr.get_u32_le();

    let _ = ptr.get_u32_le();
    let _ = ptr.get_u32_le();

    let intermediate_length = ptr.get_u32_le();
    let key = ptr.get_u32_le();
    let enc_length = ptr.get_u32_le();
    let check_sum = ptr.get_u8();
    let check_xor = ptr.get_u8();
    let version = ptr.get_u16_le();

    if version < 2 {
        decrypt_v1(
            crypted,
            width,
            height,
            bpp,
            intermediate_length,
            key,
            enc_length,
            check_sum,
            check_xor,
        )
    } else if version == 2 {
        decrypt_v2(
            crypted, width, height, bpp, key, enc_length, check_sum, check_xor,
        )
    } else {
        Err(ArcError::CbgUnsupportedVersion(version))
    }
}

/// Save decoded RGBA pixels as a PNG file.
pub fn save(data: &[u8], width: u16, height: u16, savepath: impl AsRef<Path>) -> ArcResult<()> {
    write_rgba_to_png(width, height, data, savepath.as_ref().with_extension("png"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// V1 decoder (Huffman + zero-run + reverse average sampling)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn decrypt_v1(
    crypted: &[u8],
    width: u16,
    height: u16,
    bpp: u32,
    intermediate_length: u32,
    key: u32,
    enc_length: u32,
    check_sum: u8,
    check_xor: u8,
) -> ArcResult<(Vec<u8>, u16, u16)> {
    // Data starts after the 0x30 header
    let data_start = 0x30usize;
    let mut ptr = &crypted[data_start..];

    // --- Step 1: Read and decrypt the encoded block (weight table) ---
    let enc_data = read_encoded_block(&mut ptr, key, enc_length, check_sum, check_xor)?;

    // --- Step 2: Read weight table and build Huffman tree ---
    let mut weight_ptr = &enc_data[..];
    let weights: Vec<u32> = (0..0x100).map(|_| read_variable(&mut weight_ptr)).collect();
    let tree = HuffmanTree::new_v1(&weights);

    // --- Step 3: Huffman-decompress into intermediate buffer ---
    let mut intermediate = vec![0u8; intermediate_length as usize];
    let mut bits = MsbBitStream::new(ptr);
    for dst in intermediate.iter_mut() {
        *dst = tree.decode(&mut bits) as u8;
    }

    // --- Step 4: Unpack zeros (variable-length run-length encoding) ---
    let pixel_size = (bpp / 8) as usize;
    let stride = width as usize * pixel_size;
    let mut output = vec![0u8; stride * height as usize];
    unpack_zeros(&intermediate, &mut output);

    // --- Step 5: Reverse average sampling (per-channel) ---
    reverse_average_sampling(&mut output, width as usize, height as usize, pixel_size);

    // --- Step 6: Convert to RGBA pixels ---
    let pixels = convert_to_rgba(&output, width as usize, height as usize, bpp);

    Ok((pixels, width, height))
}

/// Read the encrypted block, decrypt with key stream, and verify checksum.
fn read_encoded_block(
    ptr: &mut &[u8],
    mut key: u32,
    enc_length: u32,
    check_sum: u8,
    check_xor: u8,
) -> ArcResult<Vec<u8>> {
    let src = ptr[..enc_length as usize].as_ref();
    *ptr = &ptr[enc_length as usize..];

    let mut data = vec![0u8; enc_length as usize];
    let mut sum: u8 = 0;
    let mut xor: u8 = 0;

    for (i, byte) in data.iter_mut().enumerate() {
        *byte = src[i].wrapping_sub((hash_update(&mut key) & 0xFF) as u8);
        sum = sum.wrapping_add(*byte);
        xor ^= *byte;
    }

    if sum != check_sum || xor != check_xor {
        return Err(ArcError::CbgDecryptError);
    }

    Ok(data)
}

/// Decode variable-length zero runs into the output pixel buffer.
fn unpack_zeros(input: &[u8], output: &mut [u8]) {
    let mut src = 0usize;
    let mut dst = 0usize;
    let mut dec_zero = false;

    while dst < output.len() {
        let count = match read_variable_from_slice(input, &mut src) {
            Some(v) => v as usize,
            None => return,
        };

        if dst + count > output.len() {
            break;
        }

        if dec_zero {
            // Zero fill
            output[dst..dst + count].fill(0);
        } else {
            // Copy literal bytes
            if src + count > input.len() {
                break;
            }
            output[dst..dst + count].copy_from_slice(&input[src..src + count]);
            src += count;
        }

        dec_zero = !dec_zero;
        dst += count;
    }
}

/// Reverse the average-prediction encoding, channel by channel.
/// Ported from GARBro's ReverseAverageSampling.
fn reverse_average_sampling(output: &mut [u8], width: usize, height: usize, pixel_size: usize) {
    let stride = width * pixel_size;
    for y in 0..height {
        let line = y * stride;
        for x in 0..width {
            let pixel = line + x * pixel_size;
            for p in 0..pixel_size {
                let mut avg: i32 = 0;
                if x > 0 {
                    avg += output[pixel + p - pixel_size] as i32;
                }
                if y > 0 {
                    avg += output[pixel + p - stride] as i32;
                }
                if x > 0 && y > 0 {
                    avg /= 2;
                }
                if avg != 0 {
                    output[pixel + p] = output[pixel + p].wrapping_add(avg as u8);
                }
            }
        }
    }
}

/// Convert raw pixel data (BGR/BGRA/Gray) to RGBA.
fn convert_to_rgba(data: &[u8], width: usize, height: usize, bpp: u32) -> Vec<u8> {
    let pixel_size = (bpp / 8) as usize;
    let total = width * height;
    let mut rgba = Vec::with_capacity(total * 4);
    let mut src = 0usize;

    for _ in 0..total {
        match bpp {
            32 => {
                let b = data[src];
                let g = data[src + 1];
                let r = data[src + 2];
                let a = data[src + 3];
                rgba.extend_from_slice(&[r, g, b, a]);
            }
            24 => {
                let b = data[src];
                let g = data[src + 1];
                let r = data[src + 2];
                rgba.extend_from_slice(&[r, g, b, 0xFF]);
            }
            8 => {
                let v = data[src];
                rgba.extend_from_slice(&[v, v, v, 0xFF]);
            }
            _ => {
                // Fallback: copy raw bytes + 0xFF alpha
                for p in 0..pixel_size {
                    rgba.push(data[src + p]);
                }
                rgba.extend(iter::repeat_n(0xFF, 4 - pixel_size));
            }
        }
        src += pixel_size;
    }

    rgba
}

// ---------------------------------------------------------------------------
// V2 decoder (DCT + Huffman, ported from GARBro's ParallelCbgDecoder)
// ---------------------------------------------------------------------------
#[allow(clippy::too_many_arguments)]
fn decrypt_v2(
    crypted: &[u8],
    width: u16,
    height: u16,
    bpp: u32,
    key: u32,
    enc_length: u32,
    check_sum: u8,
    check_xor: u8,
) -> ArcResult<(Vec<u8>, u16, u16)> {
    if enc_length < 0x80 {
        return Err(ArcError::CbgDecryptError);
    }

    match bpp {
        8 | 24 | 32 => {}
        _ => return Err(ArcError::CbgDecryptError),
    }

    let data_start = 0x30usize;
    let mut ptr = &crypted[data_start..];

    // --- Step 1: Read and decrypt the DCT coefficient data (0x80 bytes) ---
    let dct_raw = read_encoded_block(&mut ptr, key, enc_length, check_sum, check_xor)?;

    // Build the DCT scaling table
    let mut dct = [[0.0f32; 64]; 2];
    for i in 0..0x80 {
        dct[i >> 6][i & 0x3F] = dct_raw[i] as f32 * DCT_TABLE[i & 0x3F];
    }

    // --- Step 2: Record base_offset BEFORE reading weight tables (matches GARBro)
    // ---
    let base_offset = ptr.as_ptr() as usize - crypted.as_ptr() as usize;

    // --- Step 3: Read the two Huffman tree weight tables ---
    let tree1 = {
        let weights = read_weight_table(&mut ptr, 0x10);
        HuffmanTree::new_v2(&weights)
    };
    let tree2 = {
        let weights = read_weight_table(&mut ptr, 0xB0);
        HuffmanTree::new_v2(&weights)
    };

    // --- Step 4: Read row-block offsets ---
    // GARBro: input_base = (int)(Input.Position + offsets.Length*4 - base_offset)
    // After reading offsets, each value minus input_base gives offset into
    // remaining_data.
    let w_align = (width as usize + 7) & !7;
    let h_align = (height as usize + 7) & !7;
    let y_blocks = h_align / 8;

    let offsets_byte_count = (y_blocks + 1) * 4;
    let current_pos = ptr.as_ptr() as usize - crypted.as_ptr() as usize;
    let input_base = (current_pos + offsets_byte_count - base_offset) as isize;

    let mut offsets = Vec::with_capacity(y_blocks + 1);
    for _ in 0..=y_blocks {
        let off = ptr.get_u32_le() as isize;
        offsets.push(off - input_base);
    }

    let remaining_data = ptr.to_vec();

    // --- Step 5: Build per-block work items ---
    let pad_skip = ((w_align >> 3) + 7) >> 3;
    let mut output = vec![0u8; w_align * h_align * 4];

    let block_params: Vec<(usize, usize, usize)> = (0..y_blocks)
        .map(|i| {
            let block_offset = (offsets[i] + pad_skip as isize) as usize;
            let next_offset = if i + 1 == y_blocks {
                remaining_data.len()
            } else {
                offsets[i + 1] as usize
            };
            let block_len = next_offset.saturating_sub(block_offset);
            let dst = i * w_align * 32;
            (block_offset, block_len, dst)
        })
        .collect();

    // --- Step 6: Parallel decode each row-block using rayon ---
    let output_mutex = std::sync::Mutex::new(&mut output);

    block_params
        .par_iter()
        .for_each(|&(block_offset, block_len, dst)| {
            if block_offset >= remaining_data.len() || block_len == 0 {
                return;
            }
            let end = (block_offset + block_len).min(remaining_data.len());
            let block_data = &remaining_data[block_offset..end];

            let mut guard = output_mutex.lock().unwrap();
            decode_block(
                block_data, &tree1, &tree2, w_align, bpp, &dct, &mut guard, dst,
            );
        });

    // --- Step 7: Decode alpha channel if 32bpp ---
    let has_alpha = if bpp == 32 && !offsets.is_empty() {
        let alpha_offset = offsets[y_blocks] as usize;
        if alpha_offset < remaining_data.len() {
            decode_alpha(
                &remaining_data[alpha_offset..],
                w_align,
                h_align,
                &mut output,
            )
        } else {
            false
        }
    } else {
        false
    };

    let pixels = crop_and_convert_v2(&output, width as usize, height as usize, w_align, has_alpha);

    Ok((pixels, width, height))
}

/// Decode a single row of 8x8 blocks.
/// Dispatches to grayscale or RGB path based on bpp.
#[allow(clippy::too_many_arguments)]
fn decode_block(
    data: &[u8],
    tree1: &HuffmanTree,
    tree2: &HuffmanTree,
    width: usize,
    bpp: u32,
    dct: &DctCoefficients,
    output: &mut [u8],
    dst_start: usize,
) {
    let mut bits = MsbBitStreamCursor::new(data);

    let block_size = match bits.read_variable() {
        Some(v) => v as usize,
        None => return,
    };

    let block_count = width / 8;

    let color_data_size = if bpp == 8 {
        block_count * 64
    } else {
        block_count * 64 * 3
    };
    let mut color_data = vec![0i16; color_data_size.max(block_size)];

    // --- DC coefficients (Tree1) ---
    let mut acc: i32 = 0;
    let mut i = 0;
    while i < block_size && !bits.is_eof() {
        let count = tree1.decode(&mut bits);
        if count != 0 {
            let v = bits.get_bits(count);
            if count > 0 && (v >> (count - 1)) == 0 {
                let v = ((-1i32) << count | v) + 1;
                acc += v;
            } else {
                acc += v;
            }
        }
        if i < color_data.len() {
            color_data[i] = acc as i16;
        }
        i += 64;
    }

    // Align to byte boundary
    if bits.cache_size & 7 != 0 {
        bits.get_bits(bits.cache_size & 7);
    }

    // --- AC coefficients (Tree2) ---
    i = 0;
    while i < block_size && !bits.is_eof() {
        let mut index: usize = 1;
        while index < 64 && !bits.is_eof() {
            let code = tree2.decode(&mut bits);
            if code == 0 {
                break;
            }
            if code == 0xF {
                index += 0x10;
                continue;
            }
            index += (code & 0xF) as usize;
            if index >= BLOCK_FILL_ORDER.len() {
                break;
            }
            let bit_count = code >> 4;
            let v = bits.get_bits(bit_count);
            let v = if bit_count != 0 && (v >> (bit_count - 1)) == 0 {
                ((-1i32) << bit_count | v) + 1
            } else {
                v
            };
            color_data[i + BLOCK_FILL_ORDER[index]] = v as i16;
            index += 1;
        }
        i += 64;
    }

    // --- IDCT + color conversion per block ---
    if bpp == 8 {
        decode_grayscale(&color_data, width, dct, output, dst_start);
    } else {
        decode_rgb(&color_data, width, dct, output, dst_start);
    }
}

/// Decode RGB blocks (24/32 bpp): 3-channel DCT + YCbCr->RGB conversion.
fn decode_rgb(
    color_data: &[i16],
    width: usize,
    dct: &DctCoefficients,
    output: &mut [u8],
    dst_start: usize,
) {
    let block_count = width / 8;
    let mut ycbr_block = [[0i16; 3]; 64];
    let mut tmp = [[0.0f32; 8]; 8];
    let mut dst = dst_start;

    for blk in 0..block_count {
        let src = blk * 64;

        for channel in 0..3 {
            decode_dct(
                channel,
                color_data,
                src + channel * width * 8,
                dct,
                &mut tmp,
                &mut ycbr_block,
            );
        }

        #[allow(clippy::needless_range_loop)]
        for j in 0..64 {
            let cy = ycbr_block[j][0] as f32;
            let cb = ycbr_block[j][1] as f32;
            let cr = ycbr_block[j][2] as f32;

            let r = cy + 1.402 * cr - 178.956;
            let g = cy - 0.34414 * cb - 0.71414 * cr + 135.95984;
            let b = cy + 1.772 * cb - 226.316;

            let y = j >> 3;
            let x = j & 7;
            let p = (y * width + x) * 4;

            if dst + p + 3 < output.len() {
                output[dst + p] = float_to_byte(b);
                output[dst + p + 1] = float_to_byte(g);
                output[dst + p + 2] = float_to_byte(r);
            }
        }

        dst += 32;
    }
}

/// Decode grayscale blocks (8 bpp): 1-channel DCT, direct output.
/// Ported from GARBro's DecodeGrayscale. Uses the same DecodeDCT as RGB
/// mode but only channel 0, and outputs raw byte values (no YCbCr->RGB).
fn decode_grayscale(
    color_data: &[i16],
    width: usize,
    dct: &DctCoefficients,
    output: &mut [u8],
    mut dst_start: usize,
) {
    let block_count = width / 8;
    let mut ycbr_block = [[0i16; 3]; 64];
    let mut tmp = [[0.0f32; 8]; 8];

    let mut src = 0usize;

    for _ in 0..block_count {
        decode_dct(0, color_data, src, dct, &mut tmp, &mut ycbr_block);
        src += 64;

        #[allow(clippy::needless_range_loop)]
        for j in 0..64 {
            let y = j >> 3;
            let x = j & 7;
            let p = (y * width + x) * 4;

            if dst_start + p + 2 < output.len() {
                let v = ycbr_block[j][0] as u8;
                output[dst_start + p] = v;
                output[dst_start + p + 1] = v;
                output[dst_start + p + 2] = v;
            }
        }

        dst_start += 32;
    }
}

/// 8x8 Inverse DCT, matching GARBro's DecodeDCT.
#[allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    clippy::too_many_arguments,
    clippy::needless_range_loop
)]
fn decode_dct(
    channel: usize,
    data: &[i16],
    src: usize,
    dct: &DctCoefficients,
    tmp: &mut [[f32; 8]; 8],
    ycbr_block: &mut [[i16; 3]; 64],
) {
    let d = if channel > 0 { 1 } else { 0 };

    for i in 0..8 {
        // Check if all AC coefficients for this column are zero
        let all_zero = data.get(src + 8 + i) == Some(&0)
            && data.get(src + 16 + i) == Some(&0)
            && data.get(src + 24 + i) == Some(&0)
            && data.get(src + 32 + i) == Some(&0)
            && data.get(src + 40 + i) == Some(&0)
            && data.get(src + 48 + i) == Some(&0)
            && data.get(src + 56 + i) == Some(&0);

        if all_zero {
            let t = data.get(src + i).copied().unwrap_or(0) as f32 * dct[d][i];
            for row in 0..8 {
                tmp[row][i] = t;
            }
            continue;
        }

        let v1 = data[src + i] as f32 * dct[d][i];
        let v2 = data[src + 8 + i] as f32 * dct[d][8 + i];
        let v3 = data[src + 16 + i] as f32 * dct[d][16 + i];
        let v4 = data[src + 24 + i] as f32 * dct[d][24 + i];
        let v5 = data[src + 32 + i] as f32 * dct[d][32 + i];
        let v6 = data[src + 40 + i] as f32 * dct[d][40 + i];
        let v7 = data[src + 48 + i] as f32 * dct[d][48 + i];
        let v8 = data[src + 56 + i] as f32 * dct[d][56 + i];

        let v10 = v1 + v5;
        let v11 = v1 - v5;
        let v12 = v3 + v7;
        let v13 = (v3 - v7) * 1.414213562f32 - v12;
        let sv1 = v10 + v12;
        let sv7 = v10 - v12;
        let sv3 = v11 + v13;
        let sv5 = v11 - v13;
        let v14 = v2 + v8;
        let v15 = v2 - v8;
        let v16 = v6 + v4;
        let v17 = v6 - v4;
        let sv8 = v14 + v16;
        let sv11 = (v14 - v16) * 1.414213562f32;
        let v9 = (v17 + v15) * 1.847759065f32;
        let sv10 = 1.082392200f32 * v15 - v9;
        let sv13 = -2.613125930f32 * v17 + v9;
        let sv6 = sv13 - sv8;
        let sv4 = sv11 - sv6;
        let sv2 = sv10 + sv4;

        tmp[0][i] = sv1 + sv8;
        tmp[1][i] = sv3 + sv6;
        tmp[2][i] = sv5 + sv4;
        tmp[3][i] = sv7 - sv2;
        tmp[4][i] = sv7 + sv2;
        tmp[5][i] = sv5 - sv4;
        tmp[6][i] = sv3 - sv6;
        tmp[7][i] = sv1 - sv8;
    }

    let mut dst = 0;
    for i in 0..8 {
        let v10 = tmp[i][0] + tmp[i][4];
        let v11 = tmp[i][0] - tmp[i][4];
        let v12 = tmp[i][2] + tmp[i][6];
        let mut v13 = tmp[i][2] - tmp[i][6];
        let v14 = tmp[i][1] + tmp[i][7];
        let v15 = tmp[i][1] - tmp[i][7];
        let v16 = tmp[i][5] + tmp[i][3];
        let v17 = tmp[i][5] - tmp[i][3];

        v13 = 1.414213562f32 * v13 - v12;
        let sv1 = v10 + v12;
        let sv7 = v10 - v12;
        let sv3 = v11 + v13;
        let sv5 = v11 - v13;
        let sv8 = v14 + v16;
        let sv11 = (v14 - v16) * 1.414213562f32;
        let v9 = (v17 + v15) * 1.847759065f32;
        let sv10 = v9 - v15 * 1.082392200f32;
        let sv13 = v9 - v17 * 2.613125930f32;
        let sv6 = sv13 - sv8;
        let sv4 = sv11 - sv6;
        let sv2 = sv10 - sv4;

        ycbr_block[dst][channel] = float_to_short(sv1 + sv8);
        dst += 1;
        ycbr_block[dst][channel] = float_to_short(sv3 + sv6);
        dst += 1;
        ycbr_block[dst][channel] = float_to_short(sv5 + sv4);
        dst += 1;
        ycbr_block[dst][channel] = float_to_short(sv7 + sv2);
        dst += 1;
        ycbr_block[dst][channel] = float_to_short(sv7 - sv2);
        dst += 1;
        ycbr_block[dst][channel] = float_to_short(sv5 - sv4);
        dst += 1;
        ycbr_block[dst][channel] = float_to_short(sv3 - sv6);
        dst += 1;
        ycbr_block[dst][channel] = float_to_short(sv1 - sv8);
        dst += 1;
    }
}

/// Decode the alpha channel (only for 32bpp V2 images).
fn decode_alpha(data: &[u8], width: usize, _height: usize, output: &mut [u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    let mut ptr = data;
    let flag = ptr.get_u32_le();
    if flag != 1 {
        return false;
    }

    let mut dst = 3usize; // first alpha byte position
    let mut ctl: u32 = 1 << 1;

    while dst < output.len() {
        ctl >>= 1;
        if ctl == 1 {
            if ptr.is_empty() {
                break;
            }
            ctl = ptr[0] as u32 | 0x100;
            ptr = &ptr[1..];
        }

        if (ctl & 1) != 0 {
            // Copy from reference
            if ptr.len() < 2 {
                break;
            }
            let v = u16::from_le_bytes([ptr[0], ptr[1]]);
            ptr = &ptr[2..];

            let x = (v & 0x3F) as i32;
            let x = if x > 0x1F { x | (!0x3F) } else { x };
            let y = ((v >> 6) & 7) as i32;
            let y = if y != 0 { y | (!7) } else { 0 };
            let count = (((v >> 9) & 0x7F) as usize) + 3;

            let src = dst as isize + (x + y * width as i32) as isize * 4;
            if src < 0 || src as usize >= dst {
                return true; // partial alpha
            }

            let mut src = src as usize;
            for _ in 0..count {
                if dst >= output.len() {
                    break;
                }
                output[dst] = output[src];
                src += 4;
                dst += 4;
            }
        } else {
            // Literal alpha value
            if ptr.is_empty() {
                break;
            }
            if dst < output.len() {
                output[dst] = ptr[0];
            }
            ptr = &ptr[1..];
            dst += 4;
        }
    }

    true
}

/// Crop the aligned-width output buffer to the actual image dimensions and
/// produce RGBA pixels (adding 0xFF alpha for non-alpha V2 images).
fn crop_and_convert_v2(
    output: &[u8],
    width: usize,
    height: usize,
    w_align: usize,
    has_alpha: bool,
) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row_start = y * w_align * 4;
        for x in 0..width {
            let src = row_start + x * 4;
            let b = output.get(src).copied().unwrap_or(0);
            let g = output.get(src + 1).copied().unwrap_or(0);
            let r = output.get(src + 2).copied().unwrap_or(0);
            let a = if has_alpha {
                output.get(src + 3).copied().unwrap_or(0xFF)
            } else {
                0xFF
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    rgba
}

// ---------------------------------------------------------------------------
// Huffman tree (generic, supports V1 and V2 building modes)
// ---------------------------------------------------------------------------

struct HuffmanNode {
    valid: bool,
    is_parent: bool,
    weight: u32,
    left: usize, // usize::MAX means none
    right: usize,
}

struct HuffmanTree {
    nodes: Vec<HuffmanNode>,
}

impl HuffmanTree {
    /// Build a Huffman tree in V1 mode (standard minimum-pair selection).
    fn new_v1(weights: &[u32]) -> Self {
        Self::build(weights, false)
    }

    /// Build a Huffman tree in V2 mode (first-child takes first-valid).
    fn new_v2(weights: &[u32]) -> Self {
        Self::build(weights, true)
    }

    fn build(weights: &[u32], v2: bool) -> Self {
        let mut nodes: Vec<HuffmanNode> = Vec::with_capacity(weights.len() * 2);
        let mut root_weight: u32 = 0;

        // Create leaf nodes
        for &w in weights {
            nodes.push(HuffmanNode {
                valid: w != 0,
                is_parent: false,
                weight: w,
                left: usize::MAX,
                right: usize::MAX,
            });
            root_weight += w;
        }

        if root_weight == 0 {
            return HuffmanTree { nodes };
        }

        loop {
            let mut child_idx = [usize::MAX; 2];
            let mut total_weight: u32 = 0;

            for i in 0..2 {
                let mut min_weight = u32::MAX;
                let mut n = 0usize;

                if v2 {
                    // V2: first, find the earliest valid node as initial candidate
                    while n < nodes.len() {
                        if nodes[n].valid {
                            min_weight = nodes[n].weight;
                            child_idx[i] = n;
                            n += 1;
                            break;
                        }
                        n += 1;
                    }
                    n = n.max(i + 1);
                }

                // Search for the minimum-weight valid node
                #[allow(clippy::needless_range_loop)]
                for j in n..nodes.len() {
                    if nodes[j].valid && nodes[j].weight < min_weight {
                        min_weight = nodes[j].weight;
                        child_idx[i] = j;
                    }
                }

                if child_idx[i] != usize::MAX {
                    nodes[child_idx[i]].valid = false;
                    total_weight += nodes[child_idx[i]].weight;
                }
            }

            nodes.push(HuffmanNode {
                valid: true,
                is_parent: true,
                weight: total_weight,
                left: child_idx[0],
                right: child_idx[1],
            });

            if total_weight >= root_weight {
                break;
            }
        }

        HuffmanTree { nodes }
    }

    /// Decode one token from the MSB bit stream.
    fn decode(&self, bits: &mut impl BitStream) -> i32 {
        let mut idx = self.nodes.len() - 1;
        while self.nodes[idx].is_parent {
            let bit = bits.get_next_bit();
            idx = if bit == 0 {
                self.nodes[idx].left
            } else {
                self.nodes[idx].right
            };
            if idx == usize::MAX {
                return 0;
            }
        }
        idx as i32
    }
}

// ---------------------------------------------------------------------------
// MSB-first bit stream (two variants: slice-based and cursor-based)
// ---------------------------------------------------------------------------

trait BitStream {
    fn get_next_bit(&mut self) -> i32;
    fn get_bits(&mut self, count: i32) -> i32;
}

/// Slice-based MSB bit stream for V1 decoding.
struct MsbBitStream<'a> {
    data: &'a [u8],
    pos: usize,
    cache: u32,
    cache_size: i32,
}

impl<'a> MsbBitStream<'a> {
    fn new(data: &'a [u8]) -> Self {
        MsbBitStream {
            data,
            pos: 0,
            cache: 0,
            cache_size: 0,
        }
    }
}

impl BitStream for MsbBitStream<'_> {
    fn get_next_bit(&mut self) -> i32 {
        if self.cache_size == 0 {
            if self.pos >= self.data.len() {
                return -1;
            }
            self.cache = self.data[self.pos] as u32;
            self.pos += 1;
            self.cache_size = 8;
        }
        let bit = ((self.cache >> 7) & 1) as i32;
        self.cache = (self.cache << 1) & 0xFF;
        self.cache_size -= 1;
        bit
    }

    fn get_bits(&mut self, mut count: i32) -> i32 {
        let mut v = 0i32;
        while count > 0 {
            let bit = self.get_next_bit();
            if bit < 0 {
                return -1;
            }
            v = (v << 1) | bit;
            count -= 1;
        }
        v
    }
}

/// Cursor-based MSB bit stream for V2 block decoding.
struct MsbBitStreamCursor<'a> {
    data: &'a [u8],
    pos: usize,
    cache: u32,
    pub cache_size: i32,
    eof: bool,
}

impl<'a> MsbBitStreamCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        MsbBitStreamCursor {
            data,
            pos: 0,
            cache: 0,
            cache_size: 0,
            eof: false,
        }
    }

    fn read_variable(&mut self) -> Option<u32> {
        let mut v = 0u32;
        let mut shift = 0;
        loop {
            let byte = self.read_byte()?;
            v |= ((byte & 0x7F) as u32) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
    }

    fn read_byte(&mut self) -> Option<u8> {
        if self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            Some(b)
        } else {
            self.eof = true;
            None
        }
    }

    /// Returns true if the underlying data has been exhausted.
    pub fn is_eof(&self) -> bool {
        self.eof
    }
}

impl BitStream for MsbBitStreamCursor<'_> {
    fn get_next_bit(&mut self) -> i32 {
        if self.cache_size == 0 {
            self.cache = self.read_byte().unwrap_or(0) as u32;
            self.cache_size = 8;
        }
        let bit = ((self.cache >> 7) & 1) as i32;
        self.cache = (self.cache << 1) & 0xFF;
        self.cache_size -= 1;
        bit
    }

    fn get_bits(&mut self, mut count: i32) -> i32 {
        let mut v = 0i32;
        while count > 0 {
            let bit = self.get_next_bit();
            if bit < 0 {
                return -1;
            }
            v = (v << 1) | bit;
            count -= 1;
        }
        v
    }
}

// ---------------------------------------------------------------------------
// Variable-length integer helpers
// ---------------------------------------------------------------------------

/// Read a variable-length integer from a byte slice, advancing the position.
/// Returns 0 if the input is exhausted mid-integer.
fn read_variable(ptr: &mut &[u8]) -> u32 {
    let mut v = 0u32;
    let mut shift = 0u32;

    loop {
        if ptr.is_empty() {
            return v;
        }
        let c = ptr[0];
        *ptr = &ptr[1..];
        v |= ((c & 0x7F) as u32) << shift;
        shift += 7;
        if c & 0x80 == 0 {
            break;
        }
    }

    v
}

/// Read a variable-length integer from a slice with an explicit index.
fn read_variable_from_slice(data: &[u8], pos: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    let mut shift = 0u32;

    loop {
        if *pos >= data.len() {
            return None;
        }
        let c = data[*pos];
        *pos += 1;
        v |= ((c & 0x7F) as u32) << shift;
        shift += 7;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
}

/// Read a weight table of `count` variable-length integers.
fn read_weight_table(ptr: &mut &[u8], count: usize) -> Vec<u32> {
    (0..count).map(|_| read_variable(ptr)).collect()
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------
#[inline]
fn float_to_short(f: f32) -> i16 {
    let a = 0x80 + (f as i32 >> 3);
    if a <= 0 {
        0
    } else if a <= 0xFF {
        a as i16
    } else if a < 0x180 {
        0xFF
    } else {
        0
    }
}

#[inline]
fn float_to_byte(f: f32) -> u8 {
    if f >= 255.0 {
        0xFF
    } else if f <= 0.0 {
        0
    } else {
        f as u8
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[allow(clippy::excessive_precision)]
const DCT_TABLE: [f32; 64] = [
    1.00000000, 1.38703990, 1.30656302, 1.17587554, 1.00000000, 0.78569496, 0.54119611, 0.27589938,
    1.38703990, 1.92387950, 1.81225491, 1.63098633, 1.38703990, 1.08979023, 0.75066054, 0.38268343,
    1.30656302, 1.81225491, 1.70710683, 1.53635550, 1.30656302, 1.02655995, 0.70710677, 0.36047992,
    1.17587554, 1.63098633, 1.53635550, 1.38268340, 1.17587554, 0.92387950, 0.63637930, 0.32442334,
    1.00000000, 1.38703990, 1.30656302, 1.17587554, 1.00000000, 0.78569496, 0.54119611, 0.27589938,
    0.78569496, 1.08979023, 1.02655995, 0.92387950, 0.78569496, 0.61731654, 0.42521504, 0.21677275,
    0.54119611, 0.75066054, 0.70710677, 0.63637930, 0.54119611, 0.42521504, 0.29289323, 0.14931567,
    0.27589938, 0.38268343, 0.36047992, 0.32442334, 0.27589938, 0.21677275, 0.14931567, 0.07612047,
];

/// Zigzag scan order for 8x8 DCT blocks.
const BLOCK_FILL_ORDER: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
