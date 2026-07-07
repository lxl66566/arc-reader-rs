use std::path::PathBuf;

use arc_reader::arc::ArcVersion;
use clap::{Parser, Subcommand};
use log::{error, info};

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

        /// Output ARC file path (optional)
        #[arg(required = false)]
        output_file: Option<PathBuf>,

        /// ARC version
        #[arg(long, short, default_value = "2", value_parser = parse_version)]
        version: ArcVersion,
    },
}

fn parse_version(v: &str) -> Result<ArcVersion, String> {
    match v {
        "1" => Ok(ArcVersion::V1),
        "2" => Ok(ArcVersion::V2),
        _ => Err("invalid version, only 1 or 2 are allowed".to_string()),
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        Commands::Unpack {
            arc_file,
            output_path,
        } => {
            let out_dir = output_path.unwrap_or(arc_file.with_extension(""));
            let results = arc_reader::unpack_arc(&arc_file, &out_dir)?;

            let errors: Vec<_> = results
                .into_iter()
                .filter_map(|(name, r)| r.err().map(|e| (name, e)))
                .collect();

            if !errors.is_empty() {
                error!("Failed to process {} files:", errors.len());
                for (name, e) in &errors {
                    error!("  - {}: {}", name, e);
                }
            }
        }
        Commands::Pack {
            input_dir,
            output_file,
            version,
        } => {
            let output = output_file.unwrap_or(input_dir.with_extension("arc"));
            arc_reader::pack_arc(&input_dir, &output, version)?;
            info!("Packed to {}", output.display());
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
        std::fs::create_dir_all(&input_dir).unwrap();
        std::fs::write(
            input_dir.join("test.ogg"),
            include_bytes!("../test_assets/test.ogg"),
        )
        .unwrap();
        run(Args {
            command: Commands::Pack {
                input_dir,
                output_file: Some(temp_dir_path.join("test.arc")),
                version: ArcVersion::V2,
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
