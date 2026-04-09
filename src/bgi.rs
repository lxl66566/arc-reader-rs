//! BGI uncompressed image format decoder.
//!
//! Ported from GARBro's ImageBGI.cs. Supports both plain and scrambled
//! (RestorePixels snake-delta) modes.

use std::path::Path;

use bytes::Buf;
use memchr::memchr;

use crate::{error::ArcResult, write::write_rgba_to_png};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check whether `data` looks like a valid BGI uncompressed image.
pub fn is_valid(data: &[u8], size: u32) -> bool {
    if size < 0x10 || data.len() < 0x10 {
        return false;
    }

    let mut ptr = data;
    let width = ptr.get_u16_le() as i32;
    if width <= 0 || width > 8096 {
        return false;
    }

    let height = ptr.get_u16_le() as i32;
    if height <= 0 || height > 8096 {
        return false;
    }

    let bpp = ptr.get_u16_le() as i32;
    if bpp != 8 && bpp != 24 && bpp != 32 {
        return false;
    }

    let flag = ptr.get_u16_le() as i32;
    if flag != 0 && flag != 1 {
        return false;
    }

    // Bytes 8..16 must be zero
    memchr(0, &data[8..16]).is_none()
}

/// Decrypt a BGI image buffer, returning (RGBA pixels, width, height).
pub fn decrypt(data: &[u8]) -> ArcResult<(Vec<u8>, u16, u16)> {
    let mut ptr = data;
    let width = ptr.get_u16_le();
    let height = ptr.get_u16_le();
    let bpp = ptr.get_u16_le() as u32;
    let flag = ptr.get_u16_le() as u32;

    let pixel_size = (bpp / 8) as usize;
    let stride = width as usize * pixel_size;
    let mut output = vec![0u8; stride * height as usize];

    if flag == 0 {
        // Plain mode: direct copy from offset 0x10
        let pixel_data = &data[0x10..];
        let copy_len = output.len().min(pixel_data.len());
        output[..copy_len].copy_from_slice(&pixel_data[..copy_len]);
    } else {
        // Scrambled mode: RestorePixels snake-delta decoding
        restore_pixels(
            &data[0x10..],
            &mut output,
            width as usize,
            height as usize,
            pixel_size,
        );
    }

    let pixels = convert_to_rgba(&output, width as usize, height as usize, bpp);
    Ok((pixels, width, height))
}

/// Save decoded RGBA pixels as a PNG file.
pub fn save(data: &[u8], width: u16, height: u16, savepath: impl AsRef<Path>) -> ArcResult<()> {
    write_rgba_to_png(width, height, data, savepath.as_ref().with_extension("png"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// RestorePixels (snake-delta decoding, ported from GARBro's ImageBGI.cs)
// ---------------------------------------------------------------------------

/// Restore scrambled pixels using snake-pattern delta decoding.
///
/// For each color channel, processes rows in pairs:
/// - Forward row (left to right): incremental delta
/// - Backward row (right to left): incremental delta
fn restore_pixels(input: &[u8], output: &mut [u8], width: usize, height: usize, bpp: usize) {
    let stride = width * bpp;
    let mut input_pos = 0usize;

    for ch in 0..bpp {
        let mut dst = ch;
        let mut incr: u8 = 0;
        let mut h = height;

        while h > 0 {
            // Forward pass (left to right)
            for _ in 0..width {
                if input_pos >= input.len() {
                    return;
                }
                incr = incr.wrapping_add(input[input_pos]);
                input_pos += 1;
                output[dst] = incr;
                dst += bpp;
            }
            h -= 1;
            if h == 0 {
                break;
            }

            // Move to end of next row
            dst += stride;

            // Backward pass (right to left)
            let mut pos = dst;
            for _ in 0..width {
                if input_pos >= input.len() {
                    return;
                }
                pos -= bpp;
                incr = incr.wrapping_add(input[input_pos]);
                input_pos += 1;
                output[pos] = incr;
            }
            h -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/// Convert raw pixel data (BGR/BGRA/Gray) to RGBA.
fn convert_to_rgba(data: &[u8], width: usize, height: usize, bpp: u32) -> Vec<u8> {
    let pixel_size = (bpp / 8) as usize;
    let total = width * height;
    let mut rgba = Vec::with_capacity(total * 4);

    for i in 0..total {
        let src = i * pixel_size;
        match bpp {
            32 => {
                // BGRX or BGRA: BGI stores as B,G,R,A
                rgba.extend_from_slice(&[
                    data[src + 2], // R
                    data[src + 1], // G
                    data[src],     // B
                    data[src + 3], // A
                ]);
            }
            24 => {
                rgba.extend_from_slice(&[
                    data[src + 2], // R
                    data[src + 1], // G
                    data[src],     // B
                    0xFF,          // A
                ]);
            }
            8 => {
                let v = data[src];
                rgba.extend_from_slice(&[v, v, v, 0xFF]);
            }
            _ => {}
        }
    }

    rgba
}
