#[allow(clippy::needless_range_loop)]
pub mod arc;
mod bse;
mod cbg;
mod decrypt;
mod dsc;
mod error;
mod write;

use clap::Parser;
use error::ArcResult;
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

fn process_file(data: &[u8], filesize: u32, filename: &str) -> ArcResult<()> {
    let mut bse_data = data.to_vec();

    if bse::is_valid(data, filesize) {
        debug!("BSE...");
        bse::decrypt(&mut bse_data)?;
        bse_data = data[16..].to_vec();
    }

    if dsc::is_valid(&bse_data, filesize) {
        debug!("DSC...");
        let (decrypted, size) = dsc::decrypt(&bse_data, filesize)?;
        dsc::save(&decrypted, size, filename)?;
    } else if cbg::is_valid(&bse_data, filesize) {
        debug!("CBG...");
        let (decrypted, w, h) = cbg::decrypt(&bse_data)?;
        cbg::save(&decrypted, w, h, filename)?;
    } else {
        debug!("uncompressed...");
        let mut file = fs::File::create(filename)?;

        file.write_all(&bse_data)?;
    }

    Ok(())
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

        let filesize = arc.get_file_size(i).map_err(|e| {
            error!("无法获取文件大小: {}", e);
            e
        })?;

        if let Err(e) = process_file(&raw_data, filesize, &file_name_with_path) {
            error!("处理文件失败: {}", e);
        }
    }

    Ok(())
}
