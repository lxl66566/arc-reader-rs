# arc-reader-rs

简体中文 | [English](./README-en.md)

这是 [minirop/arc-reader](https://github.com/minirop/arc-reader) 的 Rust 移植与改进，用于读取和提取 BGI 引擎（OverDrive/AUGUST）的 .arc 文件。

同时，本项目还额外添加了 `.ogg` 音频包的解/封包支持、图像的封包支持。

## 功能

封包/解包 .arc 文件（支持 V1 和 V2 arc 版本），支持图像 + 音频

- 解密 BSE 格式文件（仅前 64 字节加密）
- 图像解码（to PNG）：
  - CBG V1：Huffman + 零行程 + 反向平均采样
  - CBG V2：DCT + Huffman + YCbCr→RGB（8/24/32bpp，含 Alpha）+ 并行块解码
  - BGI 无压缩图像
  - DSC FORMAT 1.00
- 图像编码（from PNG）：BGI 无压缩（默认）/ CBG V1
- 音频解/封包： `.ogg` Vorbis 文件（含 BGI 音频头部）

## 用法

请在 [Release](https://github.com/lxl66566/arc-reader-rs/releases/) 下载预编译的命令行二进制文件。

```sh
arc-reader unpack <ARC_FILE> [OUTPUT_PATH]
arc-reader pack <INPUT_DIR> [OUTPUT_FILE] [-v <version>] [-i <image_format>]
```

执行 `arc-reader -h` 查看详细信息。

## 测试

`-` 为未测试，列表欢迎补充

<!-- prettier-ignore -->
| 游戏 | 版本 | 图格式 | 图解码 | 图编码 | 音频解码 | 音频编码 |
| --- | --- | --- | --- | --- | --- | --- |
| 千の刃涛、桃花染の皇姫 | v0.2.x | - | - | - | ✓ | ✓ |
| ジュエリー・ハーツ・アカデミア -We will wing wonder world- | v0.3.0 | - | ✓ | - | ✓ | ✓ |
| 大図書館の羊飼い | v0.2.x | - | - | - | ✓ | ✓ |
| 大図書館の羊飼い -Dreaming Sheep- | v0.4.0 | CBG V1 | ✓ | ✓ | ✓ | ✓ |
| 素晴らしき日々15th | v0.3.0 | - | ✓ | - | ✓ | - |

## Thanks

- [minirop/arc-reader](https://github.com/minirop/arc-reader) for the original C implementation
- [GARbro](https://github.com/nanami5270/GARbro-Mod) for the arc V2 format implementation
