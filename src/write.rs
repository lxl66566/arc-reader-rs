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

    let mut writer = encoder
        .write_header()
        .map_err(|_| ArcError::PngProcessError)?;

    writer
        .write_image_data(array)
        .map_err(|_| ArcError::PngProcessError)
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
    let mut reader = decoder.read_info().map_err(|_| ArcError::PngProcessError)?;

    let (color_type, bit_depth) = (reader.info().color_type, reader.info().bit_depth);
    if bit_depth != BitDepth::Eight {
        return Err(ArcError::PngProcessError);
    }

    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|_| ArcError::PngProcessError)?;

    let width = info.width as usize;
    let height = info.height as usize;
    let total = width * height;

    let rgba = match color_type {
        ColorType::Rgba => buf,
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
        ColorType::Indexed => return Err(ArcError::PngProcessError),
    };

    let has_alpha = rgba.chunks_exact(4).any(|px| px[3] != 0xFF);

    Ok(PngImage {
        width: width as u16,
        height: height as u16,
        rgba,
        has_alpha,
    })
}
