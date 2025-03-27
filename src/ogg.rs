use std::{
    fs::File,
    io::{Cursor, Write},
    path::Path,
};

use lewton::inside_ogg::OggStreamReader;

use crate::error::ArcResult;

/// 判断是否为 OGG 文件（带有 headers）
pub fn is_valid(data: &[u8]) -> bool {
    if data.len() < 68 {
        return false;
    }
    &data[64..68] == b"OggS"
}

/// 判断是否为 OGG 文件（不带有 headers）
pub fn is_ogg(data: &[u8]) -> bool {
    &data[0..4] == b"OggS"
}

pub fn remove_header(data: Vec<u8>) -> Vec<u8> {
    assert!(is_valid(&data));
    // 返回从第 64 字节开始的所有数据
    data[64..].to_vec()
}

pub fn add_header(data: Vec<u8>) -> Vec<u8> {
    let mut header = vec![
        0x40, 0x00, 0x00, 0x00, 0x62, 0x77, 0x20, 0x20, //
        0x00, 0x00, 0x00, 0x00, // 文件大小占位符
        0x00, 0x00, 0x00, 0x00, // 采样点数占位符
        0x44, 0xAC, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    // 计算文件大小（原始数据长度 + header长度）
    header[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());

    // 计算采样点数
    let sample_count = calculate_sample_count(&data);
    header[12..16].copy_from_slice(&sample_count.to_le_bytes());

    // 合并header和数据
    let mut result = header;
    result.extend(data);
    result
}

pub fn save(data: &[u8], savepath: impl AsRef<Path>) -> ArcResult<()> {
    let savepath = savepath.as_ref().with_extension("ogg");
    let mut file = File::create(savepath)?;
    file.write_all(data)?;
    Ok(())
}

fn calculate_sample_count(ogg_data: &[u8]) -> u32 {
    // 使用内存游标读取OGG数据
    let cursor = Cursor::new(ogg_data);
    let mut osr = OggStreamReader::new(cursor).unwrap();

    // 计算总采样点数
    let mut total_samples = 0;
    while let Ok(Some(packet)) = osr.read_dec_packet_itl() {
        total_samples += packet.len() as u32;
    }

    total_samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headers() {
        let test_ogg_data = include_bytes!("../test_assets/test.ogg");
        let test_ogg_data_with_header = add_header(test_ogg_data.to_vec());
        println!("{:02X?}", &test_ogg_data_with_header[..64]);
        assert_eq!(
            test_ogg_data_with_header[8..16],
            [0x07, 0x17, 0x00, 0x00, 0x40, 0x76, 0x00, 0x00]
        );
        let test_ogg_data_without_header = remove_header(test_ogg_data_with_header);
        assert_eq!(test_ogg_data.as_ref(), test_ogg_data_without_header);
    }
}
