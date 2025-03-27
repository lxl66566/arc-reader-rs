#[allow(clippy::needless_range_loop)]
pub mod arc;
mod bse;
mod cbg;
mod decrypt;
mod dsc;
mod error;
mod ogg;
mod write;

use std::{fs, io::Write, path::PathBuf};

use clap::Parser;
use error::ArcResult;
use log::{debug, error, info};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// ARC 文件路径
    #[arg(required = true)]
    arc_file: PathBuf,

    /// 输出目录路径（可选）
    #[arg(required = false)]
    output_path: Option<PathBuf>,
}

fn process_file(data: &[u8], filesize: u32, savepath: PathBuf) -> ArcResult<()> {
    let mut bse_data = data.to_vec();

    if bse::is_valid(data, filesize) {
        debug!("BSE...");
        bse::decrypt(&mut bse_data)?;
        bse_data = data[16..].to_vec();
    }
    if dsc::is_valid(&bse_data, filesize) {
        debug!("DSC...");
        let (decrypted, size) = dsc::decrypt(&bse_data, filesize)?;
        dsc::save(&decrypted, size, savepath)?;
    } else if cbg::is_valid(&bse_data, filesize) {
        debug!("CBG...");
        let (decrypted, w, h) = cbg::decrypt(&bse_data)?;
        cbg::save(&decrypted, w, h, savepath)?;
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    _ = pretty_env_logger::formatted_builder()
        .filter_level(log::LevelFilter::Debug)
        .format_timestamp_secs()
        .parse_default_env()
        .try_init();

    let args = Args::parse();
    let arc = arc::Arc::open(&args.arc_file)?;

    let count = arc.files_count();

    let out_dir = args.output_path.unwrap_or(args.arc_file.with_extension(""));
    if !out_dir.exists() {
        fs::create_dir_all(&out_dir)?;
    }

    info!("文件数量: {}", count);

    for i in 0..count {
        let file_name = arc.get_file_name(i).map_err(|e| {
            error!("无法获取文件名: {}", e);
            e
        })?;

        let savepath = out_dir.join(file_name);

        info!("extracting {}", file_name);

        let raw_data = arc.get_file_data(i).map_err(|e| {
            error!("无法读取文件数据: {}", e);
            e
        })?;

        let filesize = arc.get_file_size(i).map_err(|e| {
            error!("无法获取文件大小: {}", e);
            e
        })?;

        if let Err(e) = process_file(&raw_data, filesize, savepath) {
            error!("处理文件失败: {}", e);
        }
    }

    Ok(())
}
