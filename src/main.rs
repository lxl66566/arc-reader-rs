#[allow(clippy::needless_range_loop)]
pub mod arc;
mod bse;
mod cbg;
mod decrypt;
mod dsc;
mod write;

use clap::Parser;
use std::fs;
use std::io::{self, Write};
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

fn main() {
    let args = Args::parse();

    let arc = match arc::Arc::open(&args.arc_file) {
        Some(a) => a,
        None => {
            println!("无法读取文件: {}", args.arc_file);
            return;
        }
    };

    let count = arc.files_count();

    if let Some(path) = args.output_path.as_ref() {
        let path = Path::new(path);
        if !path.exists() {
            fs::create_dir_all(path).expect("无法创建目录");
        }
    }

    println!("文件数量: {}", count);

    for i in 0..count {
        let file_name_with_path = if let Some(path) = args.output_path.as_ref() {
            format!("{}/{}", path, arc.get_file_name(i))
        } else {
            arc.get_file_name(i).to_string()
        };

        print!("{}...", arc.get_file_name(i));
        io::stdout().flush().unwrap();

        let raw_data = match arc.get_file_data(i) {
            Some(data) => data,
            None => continue,
        };

        let mut bse_data = raw_data.clone();
        let filesize = arc.get_file_size(i);
        let mut good = true;

        if bse::is_valid(&raw_data, filesize) {
            print!("BSE...");
            io::stdout().flush().unwrap();
            if bse::decrypt(&mut bse_data) {
                bse_data = raw_data[16..].to_vec();
            }
        }

        if dsc::is_valid(&bse_data, filesize) {
            print!("DSC...");
            io::stdout().flush().unwrap();

            let result = dsc::decrypt(&bse_data, filesize);
            match result {
                Some((decrypted, size)) => {
                    good = dsc::save(&decrypted, size, &file_name_with_path);
                }
                None => good = false,
            }
        } else if cbg::is_valid(&bse_data, filesize) {
            print!("CBG...");
            io::stdout().flush().unwrap();

            let result = cbg::decrypt(&bse_data);
            match result {
                Some((decrypted, w, h)) => {
                    good = cbg::save(&decrypted, w, h, &file_name_with_path);
                }
                None => good = false,
            }
        } else {
            print!("uncompressed...");
            io::stdout().flush().unwrap();

            let mut file = match fs::File::create(&file_name_with_path) {
                Ok(f) => f,
                Err(_) => {
                    println!("ERROR: 无法创建文件");
                    continue;
                }
            };
            match file.write_all(&bse_data) {
                Ok(_) => (),
                Err(_) => good = false,
            }
        }

        if good {
            println!("ok");
        } else {
            println!("ERROR");
        }
    }
}
