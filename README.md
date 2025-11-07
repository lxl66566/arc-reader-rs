# arc-reader-rs

简体中文 | [English](./README-en.md)

这是 [minirop/arc-reader](https://github.com/minirop/arc-reader) 的 Rust 实现，用于读取和提取 BGI 引擎（OverDrive/MangaGamer）的 .arc 文件。这个项目从 C 语言版本的 arc-reader 移植而来，它尽可能保持了原始项目的功能和结构，但利用了 Rust 的易于构建和静态链接的特性。

同时，本项目还额外添加了 `.ogg` 音频包的解包与封包支持。其他格式暂时未支持，欢迎 PR。

## 功能

- 解包 .arc 文件
- 解密 BSE 格式文件（仅前 64 字节加密）
- 解密并保存 `CompressedBG___` 格式文件为 PNG
- 解密并保存 "DSC FORMAT 1.00" 格式文件为 PNG 或原始格式
- 支持 V1 和 V2 版本的 ARC 文件格式

## 用法

请在 [Release](https://github.com/lxl66566/arc-reader-rs/releases/) 下载预编译的命令行二进制文件。

```sh
arc-reader unpack <ARC_FILE> [OUTPUT_PATH]
arc-reader pack <INPUT_DIR> [OUTPUT_FILE] [-v <version>]
```

执行 `arc-reader -h` 查看详细信息。

## 测试

下列游戏的 .arc 存档已通过测试。

- `data04` 开头的音频存档：
  - 千の刃涛、桃花染の皇姫
  - ジュエリー・ハーツ・アカデミア -We will wing wonder world-
  - 大図書館の羊飼い <span style="color: gray;">_(arc version 1)_</span>
  - 大図書館の羊飼い -Dreaming Sheep-
- `data02` 开头的图像存档：
  - ジュエリー・ハーツ・アカデミア -We will wing wonder world-

## LICENSE

保留了[原始项目的 LICENSE](./licence.txt)。
