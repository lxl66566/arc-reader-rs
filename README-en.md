# arc-reader-rs

[简体中文](./README.md) | English

This is a Rust implementation of [minirop/arc-reader](https://github.com/minirop/arc-reader), used for reading and extracting .arc files from the BGI engine (OverDrive/MangaGamer). This project is ported from the C version of arc-reader, retaining as much of the original project's functionality and structure as possible while leveraging Rust's ease of building and static linking features.

Additionally, this project includes extra support for unpacking and repacking `.ogg` audio files. Other formats are not yet supported, and PRs are welcome.

The main contributor to this project is claude-3.7, and we extend our gratitude for their significant support.

## Features

- Unpack .arc files
- Decrypt BSE format files (only the first 64 bytes are encrypted)
- Decrypt and save `CompressedBG___` format files as PNG
- Decrypt and save "DSC FORMAT 1.00" files as PNG or raw format
- Support for both V1 and V2 ARC file formats

## Usage

Please download the precompiled command-line binary from [Release](https://github.com/lxl66566/arc-reader-rs/releases/).

```sh
arc-reader unpack <ARC_FILE> [OUTPUT_PATH]
arc-reader pack <INPUT_DIR> [OUTPUT_FILE] [-v <version>]
```

Run `arc-reader -h` for detailed information.

## LICENSE

The [original project's LICENSE](./licence.txt) is retained.
