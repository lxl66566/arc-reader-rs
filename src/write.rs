use std::{
    fs::File,
    io::{BufWriter, Cursor},
    path::Path,
};

use png::{BitDepth, ColorType};

use crate::error::{ArcError, ArcResult};

// Type aliases for better readability
type ImageWidth = u16;
type ImageHeight = u16;

/// Write RGBA pixel data to a PNG file
pub fn write_rgba_to_png(
    width: ImageWidth,
    height: ImageHeight,
    array: &[u8],
    savepath: impl AsRef<Path>,
) -> ArcResult<()> {
    let file = File::create(savepath)?;

    let w = BufWriter::new(file);

    let mut encoder = png::Encoder::new(w, u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = encoder.write_header()?;

    writer.write_image_data(array)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// PNG decoding (for packing images back into ARC archives)
// ---------------------------------------------------------------------------

/// A decoded PNG image normalized to RGBA8 pixel format.
pub struct PngImage {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    /// `true` when at least one pixel has alpha < 255 (needs 32-bit storage).
    pub has_alpha: bool,
}

/// Decode a PNG file into normalized RGBA8 pixels.
pub fn read_png(data: &[u8]) -> ArcResult<PngImage> {
    let decoder = png::Decoder::new(Cursor::new(data));
    let mut reader = decoder.read_info()?;

    let (color_type, bit_depth) = (reader.info().color_type, reader.info().bit_depth);
    if bit_depth != BitDepth::Eight {
        return Err(ArcError::PngUnsupported("only 8-bit depth is supported"));
    }
    if color_type == ColorType::Indexed {
        return Err(ArcError::PngUnsupported("indexed PNG not supported"));
    }

    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;

    let width = info.width as usize;
    let height = info.height as usize;

    let rgba = convert_png_to_rgba(&buf, width, height, color_type);

    let has_alpha = rgba.chunks_exact(4).any(|px| px[3] != 0xFF);

    let (width_u16, height_u16) = (
        u16::try_from(width).map_err(|_| ArcError::PngUnsupported("PNG width exceeds u16::MAX"))?,
        u16::try_from(height)
            .map_err(|_| ArcError::PngUnsupported("PNG height exceeds u16::MAX"))?,
    );

    Ok(PngImage {
        width: width_u16,
        height: height_u16,
        rgba,
        has_alpha,
    })
}

/// Convert PNG decoded buffer (any supported color type) to RGBA8.
fn convert_png_to_rgba(buf: &[u8], width: usize, height: usize, color_type: ColorType) -> Vec<u8> {
    let total = width * height;
    match color_type {
        ColorType::Rgba => buf.to_vec(),
        ColorType::Rgb => {
            let mut out = Vec::with_capacity(total * 4);
            for chunk in buf.chunks_exact(3).take(total) {
                out.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 0xFF]);
            }
            out
        }
        ColorType::Grayscale => {
            let mut out = Vec::with_capacity(total * 4);
            for &v in buf.iter().take(total) {
                out.extend_from_slice(&[v, v, v, 0xFF]);
            }
            out
        }
        ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(total * 4);
            for chunk in buf.chunks_exact(2).take(total) {
                out.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
            out
        }
        ColorType::Indexed => Vec::new(),
    }
}

/// Convert raw BGR/BGRA/Grayscale pixel data to RGBA.
///
/// Shared by BGI and CBG decoders. Input layout depends on `bpp`:
/// - `24`: B, G, R per pixel
/// - `32`: B, G, R, A per pixel
/// - `8`: grayscale (single channel)
/// - other: raw bytes padded with `0xFF` alpha
#[must_use]
pub fn convert_bgr_to_rgba(data: &[u8], width: usize, height: usize, bpp: u32) -> Vec<u8> {
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
                for p in 0..pixel_size {
                    rgba.push(data[src + p]);
                }
                rgba.extend(std::iter::repeat_n(0xFF, 4 - pixel_size));
            }
        }
        src += pixel_size;
    }

    rgba
}
