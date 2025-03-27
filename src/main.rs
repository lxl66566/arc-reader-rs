#[allow(clippy::needless_range_loop)]
pub mod arc;
mod bse;
mod cbg;
mod decrypt;
mod dsc;
mod error;
mod write;

use clap::Parser;
use log::{debug, error, info};
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// ARC 文件路径
    #[arg(required = true)]
    arc_file: String,

    /// 输出目录路径（可选）
    #[arg(required = false)]
    output_path: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    _ = pretty_env_logger::formatted_builder()
        .filter_level(log::LevelFilter::Debug)
        .format_timestamp_secs()
        .parse_default_env()
        .try_init();

    let args = Args::parse();
    let arc = arc::Arc::open(&args.arc_file)?;

    let count = arc.files_count();

    if let Some(path) = args.output_path.as_ref() {
        let path = Path::new(path);
        if !path.exists() {
            fs::create_dir_all(path).expect("无法创建目录");
        }
    }

    info!("文件数量: {}", count);

    for i in 0..count {
        let file_name = arc.get_file_name(i).map_err(|e| {
            error!("无法获取文件名: {}", e);
            e
        })?;

        let file_name_with_path = if let Some(path) = args.output_path.as_ref() {
            format!("{}/{}", path, file_name)
        } else {
            file_name.to_string()
        };

        info!("extracting {}...", file_name);

        let raw_data = arc.get_file_data(i).map_err(|e| {
            error!("无法读取文件数据: {}", e);
            e
        })?;

        let mut bse_data = raw_data.clone();
        let filesize = arc.get_file_size(i).map_err(|e| {
            error!("无法获取文件大小: {}", e);
            e
        })?;
        let mut good = true;

        if bse::is_valid(&raw_data, filesize) {
            debug!("BSE...");
            if bse::decrypt(&mut bse_data) {
                bse_data = raw_data[16..].to_vec();
            }
        }

        if dsc::is_valid(&bse_data, filesize) {
            debug!("DSC...");
            let result = dsc::decrypt(&bse_data, filesize);
            match result {
                Some((decrypted, size)) => {
                    good = dsc::save(&decrypted, size, &file_name_with_path);
                }
                None => good = false,
            }
        } else if cbg::is_valid(&bse_data, filesize) {
            debug!("CBG...");
            let result = cbg::decrypt(&bse_data);
            match result {
                Some((decrypted, w, h)) => {
                    good = cbg::save(&decrypted, w, h, &file_name_with_path);
                }
                None => good = false,
            }
        } else {
            debug!("uncompressed...");
            let mut file = fs::File::create(&file_name_with_path).map_err(|e| {
                error!("无法创建文件: {}", e);
                e
            })?;

            if let Err(e) = file.write_all(&bse_data) {
                error!("无法写入文件: {}", e);
                good = false;
            }
        }

        if !good {
            error!("ERROR");
        }
    }

    Ok(())
}
