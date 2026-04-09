# arc-reader-rs

[简体中文](./README.md) | English

This is a Rust port of [minirop/arc-reader](https://github.com/minirop/arc-reader) with improvements, used for reading and extracting .arc files from the BGI engine (OverDrive/MangaGamer).

Additionally, this project includes extra support for unpacking and repacking `.ogg` audio files.

## Features

- Unpack .arc files (V1 and V2 formats supported)
- Pack .arc files (currently only OGG audio packing)
- Decrypt BSE format files (only the first 64 bytes are encrypted)
- Decrypt and save `CompressedBG___` format files as PNG (V1 and V2)
  - V1: Huffman + zero-run + reverse average sampling
  - V2: DCT + Huffman + YCbCr→RGB (8/24/32bpp with alpha channel decoding) + parallel block decoding
- Decrypt and save "DSC FORMAT 1.00" files as PNG or raw format
- Decrypt and save BGI uncompressed image format as PNG
- Unpack and repack `.ogg` audio files (including BGI header)

## Usage

Please download the precompiled command-line binary from [Release](https://github.com/lxl66566/arc-reader-rs/releases/).

```sh
arc-reader unpack <ARC_FILE> [OUTPUT_PATH]
arc-reader pack <INPUT_DIR> [OUTPUT_FILE] [-v <version>]
```

Run `arc-reader -h` for detailed information.

## Tested on

Passed the test on .arc files of the following games:

- 千の刃涛、桃花染の皇姫
- ジュエリー・ハーツ・アカデミア -We will wing wonder world-
- 大図書館の羊飼い (arc version 1)
- 大図書館の羊飼い -Dreaming Sheep-

## Thanks

- [minirop/arc-reader](https://github.com/minirop/arc-reader) for the original C implementation
- [GARbro](https://github.com/nanami5270/GARbro-Mod) for the arc V2 format implementation
