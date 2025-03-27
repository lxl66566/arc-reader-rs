use std::fs::File;
use std::io::{BufWriter};
use std::path::Path;

/// 将 RGBA 数据保存为 PNG 文件
pub fn write_rgba_to_png(width: u16, height: u16, array: &[u8], filename: &str) -> bool {
    let path = Path::new(filename);
    let file = match File::create(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    
    let w = BufWriter::new(file);
    
    let mut encoder = png::Encoder::new(w, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    
    let mut writer = match encoder.write_header() {
        Ok(writer) => writer,
        Err(_) => return false,
    };
    
    writer.write_image_data(array).is_ok()
} 