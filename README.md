# arc-reader-rs

简体中文 | [English](./README-en.md)

这是 [minirop/arc-reader](https://github.com/minirop/arc-reader) 的 Rust 移植与改进，用于读取和提取 BGI 引擎（OverDrive/MangaGamer）的 .arc 文件。

同时，本项目还额外添加了 `.ogg` 音频包的解包与封包支持。

## 功能

- 解包 .arc 文件（支持 V1 和 V2 版本）
- 封包 .arc 文件（目前仅支持 OGG 音频封包）
- 解密 BSE 格式文件（仅前 64 字节加密）
- 解密并保存 `CompressedBG___` 格式文件为 PNG（支持 V1 和 V2 版本）
  - V1：Huffman + 零行程 + 反向平均采样
  - V2：DCT + Huffman + YCbCr→RGB（支持 8/24/32bpp，含 Alpha 通道解码）+ 并行块解码
- 解密并保存 "DSC FORMAT 1.00" 格式文件为 PNG 或原始格式
- 解密并保存 BGI 未压缩图像格式为 PNG
- 解包/封包 `.ogg` 音频文件（包含 BGI 音频头部）

## 用法

请在 [Release](https://github.com/lxl66566/arc-reader-rs/releases/) 下载预编译的命令行二进制文件。

```sh
arc-reader unpack <ARC_FILE> [OUTPUT_PATH]
arc-reader pack <INPUT_DIR> [OUTPUT_FILE] [-v <version>]
```

执行 `arc-reader -h` 查看详细信息。

## 测试

通过解包测试：

<!-- prettier-ignore -->
| 游戏名 | Arc 包 | 测试软件版本 |
| --- | --- | --- |
| 千の刃涛、桃花染の皇姫 | data04\* | v0.2.x |
| ジュエリー・ハーツ・アカデミア -We will wing wonder world- | data04\*<br/>data02\* | v0.3.0 |
| 大図書館の羊飼い | data04\* (arc version 1) | v0.2.x |
| 大図書館の羊飼い -Dreaming Sheep- | data04\* | v0.2.x |
| 素晴らしき日々15th | data04\*<br/>data02\* | v0.3.0 |

## Thanks

- [minirop/arc-reader](https://github.com/minirop/arc-reader) for the original C implementation
- [GARbro](https://github.com/nanami5270/GARbro-Mod) for the arc V2 format implementation
