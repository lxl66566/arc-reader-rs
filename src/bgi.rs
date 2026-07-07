//! BGI uncompressed image format decoder.
//!
//! Ported from `GARBro`'s ImageBGI.cs. Supports both plain and scrambled
//! (`RestorePixels` snake-delta) modes.

use std::path::Path;

use bytes::Buf;
use log::debug;

use crate::{
    error::ArcResult,
    write::{convert_bgr_to_rgba, write_rgba_to_png},
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check whether `data` looks like a valid BGI uncompressed image.
#[must_use]
pub fn is_bgi(data: &[u8]) -> bool {
    if data.len() < 0x10 {
        return false;
    }

    let mut ptr = data;
    let width = i32::from(ptr.get_u16_le());
    if width <= 0 || width > 8096 {
        return false;
    }

    let height = i32::from(ptr.get_u16_le());
    if height <= 0 || height > 8096 {
        return false;
    }

    let bpp = i32::from(ptr.get_u16_le());
    if bpp != 8 && bpp != 24 && bpp != 32 {
        return false;
    }

    let flag = i32::from(ptr.get_u16_le());
    if flag != 0 && flag != 1 {
        return false;
    }

    // Bytes 8..16 must be zero (reserved padding)
    data[8..16].iter().all(|&b| b == 0)
}

/// Decrypt a BGI image buffer, returning (RGBA pixels, width, height).
pub fn decrypt_bgi(data: &[u8]) -> ArcResult<(Vec<u8>, u16, u16)> {
    let mut ptr = data;
    let width = ptr.get_u16_le();
    let height = ptr.get_u16_le();
    let bpp = u32::from(ptr.get_u16_le());
    let flag = u32::from(ptr.get_u16_le());

    debug!("BGI: {width}x{height} {bpp}bpp flag={flag}");

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

    let pixels = convert_bgr_to_rgba(&output, width as usize, height as usize, bpp);
    Ok((pixels, width, height))
}

/// Save decoded RGBA pixels as a PNG file.
pub fn save(data: &[u8], width: u16, height: u16, savepath: impl AsRef<Path>) -> ArcResult<()> {
    write_rgba_to_png(width, height, data, savepath.as_ref().with_extension("png"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Encoding (PNG → BGI uncompressed image)
// ---------------------------------------------------------------------------

/// Encode RGBA pixels into a BGI uncompressed image buffer.
///
/// Uses `flag = 0` (plain mode) so no snake-delta scrambling is needed.
/// Selects 24 bpp when every pixel is opaque, otherwise 32 bpp (BGRA).
#[must_use]
pub fn encode_bgi(rgba: &[u8], width: u16, height: u16, has_alpha: bool) -> Vec<u8> {
    let bpp: u16 = if has_alpha { 32 } else { 24 };
    let pixel_size = usize::from(bpp / 8);
    let total = usize::from(width) * usize::from(height);

    let mut output = Vec::with_capacity(0x10 + total * pixel_size);

    // 16-byte header
    output.extend_from_slice(&width.to_le_bytes());
    output.extend_from_slice(&height.to_le_bytes());
    output.extend_from_slice(&bpp.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes()); // flag = 0 (plain)
    output.extend_from_slice(&[0u8; 8]); // reserved

    // RGBA → BGR(A)
    for i in 0..total {
        let off = i * 4;
        output.push(rgba[off + 2]); // B
        output.push(rgba[off + 1]); // G
        output.push(rgba[off]); // R
        if has_alpha {
            output.push(rgba[off + 3]); // A
        }
    }

    output
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

// Pixel conversion is shared via write::convert_bgr_to_rgba.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bgi_round_trip_24bpp() {
        let width = 20u16;
        let height = 16u16;
        let total = usize::from(width) * usize::from(height);

        let rgba: Vec<u8> = (0..total)
            .flat_map(|i| {
                let r = ((i * 7) % 256) as u8;
                let g = ((i * 13) % 256) as u8;
                let b = ((i * 3) % 256) as u8;
                [r, g, b, 0xFF]
            })
            .collect();

        let encoded = encode_bgi(&rgba, width, height, false);
        assert!(is_bgi(&encoded));

        let (decoded, dw, dh) = decrypt_bgi(&encoded).unwrap();
        assert_eq!(dw, width);
        assert_eq!(dh, height);
        assert_eq!(decoded, rgba);
    }

    #[test]
    fn test_bgi_round_trip_32bpp() {
        let width = 16u16;
        let height = 12u16;
        let total = usize::from(width) * usize::from(height);

        let rgba: Vec<u8> = (0..total)
            .flat_map(|i| {
                let a = if i % 2 == 0 { 0x80 } else { 0xFF };
                [
                    (i % 256) as u8,
                    ((i * 3) % 256) as u8,
                    ((i * 7) % 256) as u8,
                    a,
                ]
            })
            .collect();

        let encoded = encode_bgi(&rgba, width, height, true);
        let (decoded, _, _) = decrypt_bgi(&encoded).unwrap();
        assert_eq!(decoded, rgba);
    }
}
