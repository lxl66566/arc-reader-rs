use std::{
    fs::File,
    io::{Cursor, Write},
    path::Path,
};

use lewton::inside_ogg::OggStreamReader;

use crate::error::ArcResult;

/// Check whether this looks like a BGI-wrapped OGG/Vorbis file (bw header +
/// `OggS`).
#[must_use]
pub fn is_bgi_ogg(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }
    // Must have "bw  " at offset 4 (BGI audio signature)
    if data.len() < 8 || &data[4..8] != b"bw  " {
        return false;
    }
    // Read the Ogg data offset from the first 4 bytes
    let offset = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    if offset >= data.len() || offset + 4 > data.len() {
        return false;
    }
    &data[offset..offset + 4] == b"OggS"
}

/// Check whether this looks like a plain (header-less) OGG/Vorbis file.
#[must_use]
pub fn is_ogg(data: &[u8]) -> bool {
    data.starts_with(b"OggS")
}

#[must_use]
pub fn remove_header(data: &[u8]) -> Vec<u8> {
    assert!(is_bgi_ogg(data));
    // Read the Ogg data offset from the first 4 bytes (matches GARBro's AudioBGI)
    let offset = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    data[offset..].to_vec()
}

#[must_use]
pub fn add_header(data: &[u8]) -> Vec<u8> {
    let mut header = vec![
        0x40, 0x00, 0x00, 0x00, 0x62, 0x77, 0x20, 0x20, //
        0x00, 0x00, 0x00, 0x00, // file size placeholder
        0x00, 0x00, 0x00, 0x00, // sample count placeholder
        0x44, 0xAC, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    // Compute file size (raw data length + header length)
    header[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());

    // Compute sample count
    let sample_count = calculate_sample_count(data);
    header[12..16].copy_from_slice(&sample_count.to_le_bytes());

    // Concatenate header and data
    let mut result = header;
    result.extend_from_slice(data);
    result
}

pub fn save(data: &[u8], savepath: impl AsRef<Path>) -> ArcResult<()> {
    let savepath = savepath.as_ref().with_extension("ogg");
    let mut file = File::create(savepath)?;
    file.write_all(data)?;
    Ok(())
}

#[must_use]
pub fn calculate_sample_count(ogg_data: &[u8]) -> u32 {
    // Use memory cursor to read OGG data
    let cursor = Cursor::new(ogg_data);
    let mut osr = match OggStreamReader::new(cursor) {
        Ok(reader) => reader,
        Err(_) => return 0,
    };

    // Calculate total sample count
    let mut total_samples = 0u32;
    while let Ok(Some(packet)) = osr.read_dec_packet_itl() {
        total_samples = total_samples.saturating_add(packet.len() as u32);
    }

    total_samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headers() {
        let test_ogg_data = include_bytes!("../test_assets/test.ogg");
        let test_ogg_data_with_header = add_header(test_ogg_data);
        println!("{:02X?}", &test_ogg_data_with_header[..64]);
        assert_eq!(
            test_ogg_data_with_header[8..16],
            [0x07, 0x17, 0x00, 0x00, 0x40, 0x76, 0x00, 0x00]
        );
        let test_ogg_data_without_header = remove_header(&test_ogg_data_with_header);
        assert_eq!(test_ogg_data.as_ref(), test_ogg_data_without_header);
    }
}
