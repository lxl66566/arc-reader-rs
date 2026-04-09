use std::{fs::File, io::BufWriter, path::Path};

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

    let mut encoder = png::Encoder::new(w, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = encoder
        .write_header()
        .map_err(|_| ArcError::PngProcessError)?;

    writer
        .write_image_data(array)
        .map_err(|_| ArcError::PngProcessError)
}
