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

use arc::{V1_MAGIC, V1_METADATA_SIZE, V2_MAGIC, V2_METADATA_SIZE};
use clap::{Parser, Subcommand};
use error::{ArcError, ArcResult};
use log::{debug, error, info, warn};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 解包 ARC 文件
    Unpack {
        /// ARC 文件路径
        #[arg(required = true)]
        arc_file: PathBuf,

        /// 输出目录路径（可选）
        #[arg(required = false)]
        output_path: Option<PathBuf>,
    },
    /// 封包为 ARC 文件
    Pack {
        /// 要封包的目录路径
        #[arg(required = true)]
        input_dir: PathBuf,

        /// 输出的 ARC 文件路径
        #[arg(required = false)]
        output_file: Option<PathBuf>,

        /// ARC 版本
        #[arg(long, short, default_value = "2", value_parser = validate_version)]
        version: u8,
    },
}

fn validate_version(v: &str) -> Result<u8, String> {
    let v = v.parse::<u8>().map_err(|_| "无效的版本")?;
    if v == 1 || v == 2 {
        Ok(v)
    } else {
        Err("无效的版本，可选值为 1 或 2".to_string())
    }
}

fn unpack_file(data: &[u8], filesize: u32, savepath: PathBuf) -> ArcResult<()> {
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

// 写入文件名的辅助函数，用于封包
fn write_filename(arc_file: &mut impl std::io::Write, file_name: &str) -> std::io::Result<()> {
    let mut name_bytes = [0u8; 16];
    let name_len = file_name.len().min(16);
    name_bytes[..name_len].copy_from_slice(&file_name.as_bytes()[..name_len]);
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

                if let Err(e) = unpack_file(&raw_data, filesize, savepath) {
                    error!("处理文件失败: {}", e);
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

            // 遍历目录中的所有文件
            for entry in fs::read_dir(input_dir)? {
                let entry = entry?;
                let path = entry.path();

                if !path.is_file() {
                    warn!("{} 不是文件，跳过处理", path.display());
                    continue;
                }

                info!("adding file: {}", path.display());

                let temp_path = path.with_extension("");

                let file_name = temp_path
                    .file_name()
                    .ok_or("无效的文件名")?
                    .to_str()
                    .ok_or("无效的文件名编码")?;

                // 读取文件内容
                let mut data = fs::read(&path)?;

                // 如果是 OGG 文件，添加头部
                if ogg::is_ogg(&data) {
                    data = ogg::add_header(data);
                } else {
                    error!("暂不支持该文件类型，欢迎 PR");
                    return Err(Box::new(ArcError::UnsupportedFileType(
                        path.display().to_string(),
                    )));
                }

                // 将文件名和数据添加到列表中
                files.push((file_name.to_string(), data));
            }

            // 创建 ARC 文件
            let mut arc_file = fs::File::create(output_file)?;

            let (magic, metadata_size) = match version {
                1 => (V1_MAGIC, V1_METADATA_SIZE),
                2 => (V2_MAGIC, V2_METADATA_SIZE),
                _ => unreachable!("没有这个版本"),
            };

            // 写入魔数和文件数量
            arc_file.write_all(magic)?;
            arc_file.write_all(&(files.len() as u32).to_le_bytes())?;

            // 计算数据段起始位置
            let _header_size = 16u32 + (files.len() as u32 * metadata_size);
            let mut current_offset = 0u32;

            // 写入文件元数据
            for (file_name, data) in &files {
                write_filename(&mut arc_file, file_name)?;

                // 版本特定填充
                if version == 2 {
                    arc_file.write_all(&[0u8; 80])?; // 20 * 4 bytes
                }

                arc_file.write_all(&current_offset.to_le_bytes())?;
                arc_file.write_all(&(data.len() as u32).to_le_bytes())?;

                // 版本特定尾部填充
                arc_file.write_all(match version {
                    1 => &[0u8; 8],
                    2 => &[0u8; 24],
                    _ => unreachable!("没有这个版本"),
                })?;

                current_offset += data.len() as u32;
            }

            // 写入文件数据
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
