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
        debug!("BSE...");
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
        debug!("CBG...");
        let (decrypted, w, h) = cbg::decrypt_cbg(&bse_data)?;
        cbg::save(&decrypted, w, h, output_path)?;
    } else if bgi::is_bgi(&bse_data) {
        debug!("BGI...");
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

        // Only OGG files are supported for packing
        if ogg::is_ogg(&data) {
            data = ogg::add_header(&data);
        } else {
            error!("Unsupported file type: {}", path.display());
            return Err(ArcError::UnsupportedFileType(path.display().to_string()));
        }

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
