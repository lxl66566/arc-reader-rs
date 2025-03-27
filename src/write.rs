use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use crate::error::{ArcError, ArcResult};

/// 将 RGBA 数据保存为 PNG 文件
pub fn write_rgba_to_png(width: u16, height: u16, array: &[u8], filename: &str) -> ArcResult<()> {
    let path = Path::new(filename);
    let file = File::create(path)?;

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
