#![warn(clippy::cargo)]

pub mod arc;
pub mod bgi;
pub mod bse;
pub mod cbg;
pub mod dsc;
pub mod error;
pub mod ogg;
pub mod write;

pub(crate) mod decrypt;

use std::{fs, io::Write, path::Path};

use log::{debug, error, info, warn};
use rayon::prelude::*;

use crate::{
    arc::ArcVersion,
    error::{ArcError, ArcResult},
};

/// Check whether the data starts with a PNG magic signature.
fn is_png(data: &[u8]) -> bool {
    data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
}

/// Encode a single file for inclusion in an ARC archive.
///
/// - **OGG** → BGI-wrapped audio (`bw  ` header)
/// - **PNG** → CBG V1 compressed image, falling back to BGI uncompressed on the
///   rare occasion that Huffman code lengths are pathological.
fn encode_for_pack(data: &[u8]) -> ArcResult<Vec<u8>> {
    if ogg::is_ogg(data) {
        Ok(ogg::add_header(data))
    } else if is_png(data) {
        let img = write::read_png(data)?;
        match cbg::encode_cbg_v1(&img.rgba, img.width, img.height, img.has_alpha) {
            Ok(cbg_data) => Ok(cbg_data),
            Err(e) => {
                debug!("CBG V1 encode failed ({e:?}), falling back to BGI uncompressed");
                Ok(bgi::encode_bgi(
                    &img.rgba,
                    img.width,
                    img.height,
                    img.has_alpha,
                ))
            }
        }
    } else {
        Err(ArcError::UnsupportedFileType("unknown format".to_string()))
    }
}

/// Decode a single file extracted from an ARC archive.
///
/// Automatically detects and handles:
/// - BSE stream encryption (transparently decrypted)
/// - DSC FORMAT 1.00 compressed data (→ PNG if image, else raw)
/// - `CompressedBG` (CBG) V1/V2 images (→ PNG)
/// - BGI uncompressed images (→ PNG)
/// - BGI-wrapped OGG Vorbis audio (→ OGG)
/// - Unrecognized data (written as-is)
pub fn decode_file(data: &[u8], output_path: impl AsRef<Path>) -> ArcResult<()> {
    // BSE wraps the inner file.  Only the 0x40-byte header at offsets 0x10..0x4F
    // is encrypted; the body (from 0x50) is plaintext.
    // After stripping the 0x10-byte BSE metadata, the inner payload is:
    //   decrypted_header (0x40 bytes) + body (rest)
    let bse_data = if bse::is_bse(data) {
        let mut payload = data.to_vec();
        bse::decrypt_bse(&mut payload)?;
        payload[0x10..].to_vec()
    } else {
        data.to_vec()
    };

    if dsc::is_dsc(&bse_data) {
        debug!("DSC...");
        let (decrypted, size) = dsc::decrypt_dsc(&bse_data)?;
        dsc::save(&decrypted, size, output_path)?;
    } else if cbg::is_cbg(&bse_data) {
        let (decrypted, w, h) = cbg::decrypt_cbg(&bse_data)?;
        cbg::save(&decrypted, w, h, output_path)?;
    } else if bgi::is_bgi(&bse_data) {
        let (decrypted, w, h) = bgi::decrypt_bgi(&bse_data)?;
        bgi::save(&decrypted, w, h, output_path)?;
    } else if ogg::is_bgi_ogg(&bse_data) {
        debug!("OGG...");
        let header_removed = ogg::remove_header(&bse_data);
        ogg::save(&header_removed, output_path)?;
    } else {
        debug!("uncompressed...");
        let mut file = fs::File::create(output_path.as_ref())?;
        file.write_all(&bse_data)?;
    }

    Ok(())
}

/// Unpack all entries from an ARC archive into a directory.
///
/// Returns a list of `(filename, result)` for each processed entry.
pub fn unpack_arc(
    arc_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> ArcResult<Vec<(String, ArcResult<()>)>> {
    let arc = crate::arc::Arc::open(arc_path.as_ref())?;
    let count = arc.files_count();
    let out_dir = output_dir.as_ref();

    if !out_dir.exists() {
        fs::create_dir_all(out_dir)?;
    }

    info!("File count: {count}");

    // Phase 1: sequential I/O only — File::try_clone() shares the underlying
    // file pointer on all platforms, so concurrent seek+read causes data races
    // (wrong offsets → corrupted output, or premature EOF → "failed to fill
    // whole buffer").  Read everything into memory first, then decode in parallel.
    let file_infos: Vec<(String, ArcResult<Vec<u8>>)> = (0..count)
        .map(|i| {
            let file_name = match arc.get_file_name(i) {
                Ok(n) => n.to_string(),
                Err(e) => {
                    error!("Failed to get file name at index {i}: {e}");
                    return (format!("<index {i}>"), Err(e));
                }
            };
            match arc.get_file_data(i) {
                Ok(d) => (file_name, Ok(d)),
                Err(e) => {
                    error!("Failed to read data for {file_name}: {e}");
                    (file_name, Err(e))
                }
            }
        })
        .collect();

    let results: Vec<(String, ArcResult<()>)> = file_infos
        .into_par_iter()
        .map(|(file_name, data)| {
            let data = match data {
                Ok(d) => d,
                Err(e) => return (file_name, Err(e)),
            };
            info!("Extracting {file_name}");
            let result = decode_file(&data, out_dir.join(&file_name));
            if let Err(ref e) = result {
                error!("Failed to process file {file_name}: {e}");
            }
            (file_name, result)
        })
        .collect();

    Ok(results)
}

/// Pack files from a directory into an ARC archive (V1 or V2).
///
/// Currently only OGG audio files are supported; each file's extension-less
/// name is used as the ARC entry name, and a BGI audio header is prepended.
pub fn pack_arc(
    input_dir: impl AsRef<Path>,
    output_file: impl AsRef<Path>,
    version: ArcVersion,
) -> ArcResult<()> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    for entry in fs::read_dir(input_dir.as_ref())? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            warn!("{} is not a file, skipping", path.display());
            continue;
        }

        info!("adding file: {}", path.display());

        // Use filename without extension as the ARC entry name
        let temp_path = path.with_extension("");
        let file_name = temp_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or(ArcError::InvalidFormat)?;

        let mut data = fs::read(&path)?;

        data = encode_for_pack(&data)?;

        files.push((file_name, data));
    }

    // Write ARC archive
    let mut arc_file = fs::File::create(output_file.as_ref())?;

    // Write header: magic (12 bytes) + file count (4 bytes)
    arc_file.write_all(version.magic())?;
    arc_file.write_all(&(files.len() as u32).to_le_bytes())?;

    let mut current_offset = 0u32;

    // Write per-file metadata entries.
    // V1: [16-byte name][4 offset][4 size][8 padding] = 32 bytes
    // V2: [96-byte name][4 offset][4 size][24 padding] = 128 bytes
    let name_len_limit = version.name_len();
    let padding = version.metadata_size() as usize - 8 - name_len_limit;

    for (file_name, data) in &files {
        write_filename(&mut arc_file, file_name, name_len_limit)?;

        arc_file.write_all(&current_offset.to_le_bytes())?;
        arc_file.write_all(&(data.len() as u32).to_le_bytes())?;

        // Version-specific trailing padding
        arc_file.write_all(&vec![0u8; padding])?;

        current_offset += data.len() as u32;
    }

    // Write raw file data
    for (_, data) in files {
        arc_file.write_all(&data)?;
    }

    Ok(())
}

/// Write a null-padded filename into an ARC metadata entry.
fn write_filename(
    arc_file: &mut impl Write,
    file_name: &str,
    name_len_limit: usize,
) -> std::io::Result<()> {
    let mut name_bytes = vec![0u8; name_len_limit];
    let copy_len = file_name.len().min(name_len_limit);
    name_bytes[..copy_len].copy_from_slice(&file_name.as_bytes()[..copy_len]);
    arc_file.write_all(&name_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode RGBA as an in-memory PNG file (for testing the full pipeline).
    fn make_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        let w = std::io::BufWriter::new(&mut buf);
        let mut enc = png::Encoder::new(w, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(rgba).unwrap();
        drop(writer);
        buf
    }

    /// Full pipeline: PNG → encode_for_pack → decode_file → compare RGBA
    /// pixels.
    fn assert_round_trip(rgba: &[u8], width: u16, height: u16, _has_alpha: bool) {
        let png_data = make_png(rgba, width as u32, height as u32);
        assert!(is_png(&png_data));

        let encoded = encode_for_pack(&png_data).unwrap();

        // decode_file writes to disk; test the decoder directly instead.
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("output.png");
        decode_file(&encoded, &out).unwrap();

        let decoded_png = std::fs::read(&out).unwrap();
        let img = write::read_png(&decoded_png).unwrap();
        assert_eq!(img.width, width);
        assert_eq!(img.height, height);
        assert_eq!(img.rgba, rgba);
    }

    #[test]
    fn test_png_cbg_v1_pipeline() {
        let (w, h) = (40u16, 30u16);
        let total = usize::from(w) * usize::from(h);
        let rgba: Vec<u8> = (0..total)
            .flat_map(|i| {
                [
                    ((i * 7) % 256) as u8,
                    ((i * 13 + 50) % 256) as u8,
                    ((i * 3 + 100) % 256) as u8,
                    0xFF,
                ]
            })
            .collect();
        assert_round_trip(&rgba, w, h, false);
    }

    #[test]
    fn test_png_bgi_pipeline() {
        // Force BGI path by encoding directly
        let (w, h) = (16u16, 12u16);
        let total = usize::from(w) * usize::from(h);
        let rgba: Vec<u8> = (0..total)
            .flat_map(|i| {
                [
                    (i % 256) as u8,
                    ((i * 5) % 256) as u8,
                    ((i * 9) % 256) as u8,
                    0xFF,
                ]
            })
            .collect();

        let encoded = bgi::encode_bgi(&rgba, w, h, false);
        assert!(bgi::is_bgi(&encoded));

        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("bgi_test.png");
        decode_file(&encoded, &out).unwrap();

        let decoded = std::fs::read(&out).unwrap();
        let img = write::read_png(&decoded).unwrap();
        assert_eq!(img.rgba, rgba);
    }

    #[test]
    fn test_pack_unpack_arc_with_image() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();

        let input_dir = base.join("input");
        std::fs::create_dir_all(&input_dir).unwrap();

        // Create a test PNG
        let (w, h) = (24u16, 24u16);
        let total = usize::from(w) * usize::from(h);
        let rgba: Vec<u8> = (0..total)
            .flat_map(|i| {
                [
                    ((i * 11) % 256) as u8,
                    ((i * 7) % 256) as u8,
                    ((i * 3) % 256) as u8,
                    0xFF,
                ]
            })
            .collect();
        let png_data = make_png(&rgba, w as u32, h as u32);
        std::fs::write(input_dir.join("test.png"), &png_data).unwrap();

        // Pack
        let arc_path = base.join("test.arc");
        pack_arc(&input_dir, &arc_path, ArcVersion::V2).unwrap();

        // Unpack
        let output_dir = base.join("output");
        let results = unpack_arc(&arc_path, &output_dir).unwrap();
        assert!(results.iter().all(|(_, r)| r.is_ok()));

        // Verify the decoded PNG
        let decoded_png = std::fs::read(output_dir.join("test.png")).unwrap();
        let img = write::read_png(&decoded_png).unwrap();
        assert_eq!(img.width, w);
        assert_eq!(img.height, h);
        assert_eq!(img.rgba, rgba);
    }
}
