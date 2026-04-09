#[allow(clippy::needless_range_loop)]
pub mod arc;
mod bgi;
mod bse;
mod cbg;
mod decrypt;
mod dsc;
mod error;
mod ogg;
mod write;

use std::{fs, io::Write, path::PathBuf, sync::Mutex};

use arc::{V1_MAGIC, V1_METADATA_SIZE, V2_MAGIC, V2_METADATA_SIZE};
use clap::{Parser, Subcommand};
use error::{ArcError, ArcResult};
use log::{debug, error, info, warn};
use rayon::prelude::*;

use crate::arc::{V1_NAME_LEN, V2_NAME_LEN};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Unpack ARC file
    Unpack {
        /// Path to ARC file
        #[arg(required = true)]
        arc_file: PathBuf,

        /// Output directory path (optional)
        #[arg(required = false)]
        output_path: Option<PathBuf>,
    },
    /// Pack directory into ARC file
    Pack {
        /// Path to directory to pack
        #[arg(required = true)]
        input_dir: PathBuf,

        /// Output ARC file path
        #[arg(required = false)]
        output_file: Option<PathBuf>,

        /// ARC version
        #[arg(long, short, default_value = "2", value_parser = validate_version)]
        version: u8,
    },
}

fn validate_version(v: &str) -> Result<u8, String> {
    let v = v.parse::<u8>().map_err(|_| "Invalid version")?;
    if v == 1 || v == 2 {
        Ok(v)
    } else {
        Err("Invalid version, only 1 or 2 are allowed".to_string())
    }
}

fn unpack_file(data: &[u8], filesize: u32, savepath: PathBuf) -> ArcResult<()> {
    // BSE wraps the inner file.  Only the 0x40-byte header at offsets 0x10..0x4F
    // is encrypted; the body (from 0x50) is plaintext.
    // After stripping the 0x10-byte BSE metadata, the inner payload is:
    //   decrypted_header (0x40 bytes) + body (rest)
    let bse_data = if bse::is_valid(data, filesize) {
        debug!("BSE...");
        let mut payload = data.to_vec();
        bse::decrypt(&mut payload)?;
        // Strip the 0x10-byte BSE metadata; the decrypted header + body remain
        payload[0x10..].to_vec()
    } else {
        data.to_vec()
    };

    if dsc::is_valid(&bse_data, filesize) {
        debug!("DSC...");
        let (decrypted, size) = dsc::decrypt(&bse_data, filesize)?;
        dsc::save(&decrypted, size, savepath)?;
    } else if cbg::is_valid(&bse_data, filesize) {
        debug!("CBG...");
        let (decrypted, w, h) = cbg::decrypt(&bse_data)?;
        cbg::save(&decrypted, w, h, savepath)?;
    } else if bgi::is_valid(&bse_data, filesize) {
        debug!("BGI...");
        let (decrypted, w, h) = bgi::decrypt(&bse_data)?;
        bgi::save(&decrypted, w, h, savepath)?;
    } else if ogg::is_valid(&bse_data) {
        debug!("OGG...");
        let header_removed = ogg::remove_header(bse_data);
        ogg::save(&header_removed, savepath)?;
    } else {
        debug!("uncompressed...");
        let mut file = fs::File::create(savepath)?;

        file.write_all(&bse_data)?;
    }

    Ok(())
}

// Write filename helper for pack operations.
// V1: 16 bytes, V2: 96 bytes (matching GARBro's 0x60 name field).
fn write_filename(
    arc_file: &mut impl std::io::Write,
    file_name: &str,
    version: u8,
) -> std::io::Result<()> {
    let name_len_limit = if version == 1 {
        V1_NAME_LEN
    } else {
        V2_NAME_LEN
    };
    let mut name_bytes = vec![0u8; name_len_limit];
    let copy_len = file_name.len().min(name_len_limit);
    name_bytes[..copy_len].copy_from_slice(&file_name.as_bytes()[..copy_len]);
    arc_file.write_all(&name_bytes)
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        Commands::Unpack {
            arc_file,
            output_path,
        } => {
            let arc = arc::Arc::open(&arc_file)?;
            let count = arc.files_count();

            let out_dir = output_path.unwrap_or(arc_file.with_extension(""));
            if !out_dir.exists() {
                fs::create_dir_all(&out_dir)?;
            }

            info!("File count: {}", count);

            // Collect file info for parallel processing
            let file_infos: Vec<(u32, String, Vec<u8>, u32)> = (0..count)
                .filter_map(|i| {
                    let file_name = arc.get_file_name(i).ok()?;
                    let raw_data = arc.get_file_data(i).ok()?;
                    let filesize = arc.get_file_size(i).ok()?;
                    Some((i, file_name.to_string(), raw_data, filesize))
                })
                .collect();

            // Process files in parallel
            let errors = Mutex::new(Vec::new());
            file_infos
                .par_iter()
                .for_each(|(i, file_name, raw_data, filesize)| {
                    info!("Extracting {}", file_name);
                    let savepath = out_dir.join(file_name);
                    if let Err(e) = unpack_file(raw_data, *filesize, savepath) {
                        error!("Failed to process file {}: {}", file_name, e);
                        errors.lock().unwrap().push((*i, file_name.clone(), e));
                    }
                });

            // Report any errors
            let errors = errors.into_inner().unwrap();
            if !errors.is_empty() {
                error!("Failed to process {} files:", errors.len());
                for (_, name, e) in &errors {
                    error!("  - {}: {}", name, e);
                }
            }
        }
        Commands::Pack {
            input_dir,
            output_file,
            version,
        } => {
            let output_file = output_file.unwrap_or(input_dir.with_extension("arc"));
            let mut files = Vec::new();

            // Iterate through all files in the directory
            for entry in fs::read_dir(input_dir)? {
                let entry = entry?;
                let path = entry.path();

                if !path.is_file() {
                    warn!("{} is not a file, skipping", path.display());
                    continue;
                }

                info!("adding file: {}", path.display());

                let temp_path = path.with_extension("");

                let file_name = temp_path
                    .file_name()
                    .ok_or("Invalid filename")?
                    .to_str()
                    .ok_or("Invalid filename encoding")?;

                // Read file content
                let mut data = fs::read(&path)?;

                // If it's an OGG file, add header
                if ogg::is_ogg(&data) {
                    data = ogg::add_header(data);
                } else {
                    error!("Unsupported file type");
                    return Err(Box::new(ArcError::UnsupportedFileType(
                        path.display().to_string(),
                    )));
                }

                // Add filename and data to the list
                files.push((file_name.to_string(), data));
            }

            // Create ARC file
            let mut arc_file = fs::File::create(output_file)?;

            let (magic, metadata_size) = match version {
                1 => (V1_MAGIC, V1_METADATA_SIZE),
                2 => (V2_MAGIC, V2_METADATA_SIZE),
                _ => unreachable!("No such version"),
            };

            // Write magic number and file count
            arc_file.write_all(magic)?;
            arc_file.write_all(&(files.len() as u32).to_le_bytes())?;

            // Calculate data section start position
            let _header_size = 16u32 + (files.len() as u32 * metadata_size);
            let mut current_offset = 0u32;

            // Write file metadata entries.
            // V1: [16 name][4 offset][4 size][8 padding] = 32 bytes
            // V2: [96 name][4 offset][4 size][24 padding] = 128 bytes
            for (file_name, data) in &files {
                write_filename(&mut arc_file, file_name, version)?;

                arc_file.write_all(&current_offset.to_le_bytes())?;
                arc_file.write_all(&(data.len() as u32).to_le_bytes())?;

                // Version-specific trailing padding
                arc_file.write_all(match version {
                    1 => &[0u8; 8],
                    2 => &[0u8; 24],
                    _ => unreachable!("invalid version"),
                })?;

                current_offset += data.len() as u32;
            }

            // Write file data
            for (_, data) in files {
                arc_file.write_all(&data)?;
            }
        }
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    _ = pretty_env_logger::formatted_builder()
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .parse_default_env()
        .try_init();

    let args = Args::parse();
    run(args)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_run() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path();

        let input_dir = temp_dir_path.join("input");
        fs::create_dir_all(&input_dir).unwrap();
        fs::write(
            input_dir.join("test.ogg"),
            include_bytes!("../test_assets/test.ogg"),
        )
        .unwrap();
        run(Args {
            command: Commands::Pack {
                input_dir,
                output_file: Some(temp_dir_path.join("test.arc")),
                version: 2,
            },
        })
        .unwrap();

        assert!(temp_dir_path.join("test.arc").exists());

        run(Args {
            command: Commands::Unpack {
                arc_file: temp_dir_path.join("test.arc"),
                output_path: Some(temp_dir_path.join("output")),
            },
        })
        .unwrap();

        assert!(temp_dir_path.join("output").exists());
        assert!(temp_dir_path.join("output/test.ogg").exists());
    }
}
