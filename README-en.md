# arc-reader-rs

[简体中文](./README.md) | English

This is a Rust port of [minirop/arc-reader](https://github.com/minirop/arc-reader) with improvements, used for reading and extracting .arc files from the BGI engine (OverDrive/AUGUST).

Additionally, this project adds extra support for unpacking/packing `.ogg` audio files and image packing.

## Features

Pack/unpack .arc files (V1 and V2 arc versions supported), supports images + audio

- Decrypt BSE format files (only first 64 bytes encrypted)
- Image decoding (to PNG):
  - CBG V1: Huffman + zero-run + reverse average sampling
  - CBG V2: DCT + Huffman + YCbCr→RGB (8/24/32bpp, with Alpha) + parallel block decoding
  - BGI uncompressed images
  - DSC FORMAT 1.00
- Image encoding (from PNG): BGI uncompressed (default) / CBG V1
- Audio unpack/pack: `.ogg` Vorbis files (with BGI audio header)

## Download

Choose one of the following:

- Download the precompiled command-line binary from [Release](https://github.com/lxl66566/arc-reader-rs/releases/).
- Install via [cargo-binstall](https://github.com/cargo-bins/cargo-binstall): `cargo binstall arc-reader -y`
- Install via [bpm](https://github.com/lxl66566/bpm-rs): `bpm i https://github.com/lxl66566/arc-reader-rs`

## Usage

```sh
arc-reader unpack <ARC_FILE> [OUTPUT_PATH]
arc-reader pack <INPUT_DIR> [OUTPUT_FILE] [-v <version>] [-i <image_format>]
```

Run `arc-reader -h` for detailed information.

## Tested on

`-` means untested, contributions to the list are welcome.

<!-- prettier-ignore -->
| Game | Ver | Img Fmt | Img Dec | Img Enc | Aud Dec | Aud Enc |
| --- | --- | --- | --- | --- | --- | --- |
| 千の刃涛、桃花染の皇姫 | v0.2.x | - | - | - | ✓ | ✓ |
| ジュエリー・ハーツ・アカデミア -We will wing wonder world- | v0.4.0 | CBG V2 | ✓ | ✓ [^1] | ✓ | ✓ |
| 大図書館の羊飼い | v0.2.x | - | - | - | ✓ | ✓ |
| 大図書館の羊飼い -Dreaming Sheep- | v0.4.0 | CBG V1 | ✓ | ✓ | ✓ | ✓ |
| 素晴らしき日々15th | v0.4.0 | CBG V2 | ✓ | - | ✓ | - |

[^1]: Packing with CBG V1 format allows the game engine to read and play normally.

## Thanks

- [minirop/arc-reader](https://github.com/minirop/arc-reader) for the original C implementation
- [GARbro](https://github.com/nanami5270/GARbro-Mod) for the arc V2 format implementation
