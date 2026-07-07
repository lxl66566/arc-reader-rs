// BGI audio wrapper header size is u32; files > 4 GB are unsupported.
#![allow(clippy::cast_possible_truncation)]

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
    // BGI audio wrapper header (matches GARBro's AudioBGI):
    //   offset 0..4   : data offset (always 0x40)
    //   offset 4..8   : "bw  " signature
    //   offset 8..12  : wrapped file size
    //   offset 12..16 : total PCM sample count
    //   offset 16..20 : sample rate   (filled from the vorbis ident header)
    //   offset 20..24 : channel count (filled from the vorbis ident header)
    //   offset 48..52 : unknown constant (0x03)
    let mut header = vec![
        0x40, 0x00, 0x00, 0x00, 0x62, 0x77, 0x20, 0x20, //
        0x00, 0x00, 0x00, 0x00, // file size placeholder
        0x00, 0x00, 0x00, 0x00, // sample count placeholder
        0x00, 0x00, 0x00, 0x00, // sample rate placeholder
        0x00, 0x00, 0x00, 0x00, // channels placeholder
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    // Pull sample rate, channel count, and PCM sample count from the stream.
    let meta = read_vorbis_meta(data);
    header[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
    header[12..16].copy_from_slice(&meta.sample_count.to_le_bytes());
    header[16..20].copy_from_slice(&meta.sample_rate.to_le_bytes());
    header[20..24].copy_from_slice(&u32::from(meta.channels).to_le_bytes());

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
    read_vorbis_meta(ogg_data).sample_count
}

/// Metadata extracted from an OGG/Vorbis stream by [`read_vorbis_meta`].
#[derive(Default)]
struct VorbisMeta {
    sample_rate: u32,
    channels: u8,
    sample_count: u32,
}

/// Decode an OGG/Vorbis blob just enough to read its identification header
/// (sample rate, channel count) and count its decoded PCM samples.
///
/// Returns a zeroed [`VorbisMeta`] if the data is not a valid OGG/Vorbis
/// stream, so callers can fill the wrapper header defensively.
fn read_vorbis_meta(ogg_data: &[u8]) -> VorbisMeta {
    let cursor = Cursor::new(ogg_data);
    let Ok(mut osr) = OggStreamReader::new(cursor) else {
        return VorbisMeta::default();
    };

    let sample_rate = osr.ident_hdr.audio_sample_rate;
    let channels = osr.ident_hdr.audio_channels;

    // Calculate total sample count
    let mut total_samples = 0u32;
    while let Ok(Some(packet)) = osr.read_dec_packet_itl() {
        total_samples = total_samples.saturating_add(packet.len() as u32);
    }

    VorbisMeta {
        sample_rate,
        channels,
        sample_count: total_samples,
    }
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
        // Sample rate (44100) and channel count (1) read from the vorbis stream
        assert_eq!(
            test_ogg_data_with_header[16..24],
            [0x44, 0xAC, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]
        );
        let test_ogg_data_without_header = remove_header(&test_ogg_data_with_header);
        assert_eq!(test_ogg_data.as_ref(), test_ogg_data_without_header);
    }
}
