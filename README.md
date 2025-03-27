# arc-reader-rs

这是 [minirop/arc-reader](https://github.com/minirop/arc-reader) 的 Rust 实现，用于读取和提取 BGI 引擎（OverDrive/MangaGamer）的 .arc 文件。这个项目从 C 语言版本的 arc-reader 移植而来，它保持了原始项目的功能和结构，但利用了 Rust 的易于构建和静态链接的特性。

本项目的主要编写者为 claude-3.7，感谢其对本项目的大力支持。

## 功能

- 解压 .arc 文件
- 解密 BSE 格式文件（仅前 64 字节加密）
- 解密并保存 "CompressedBG\_\_\_" 格式文件为 PNG
- 解密并保存 "DSC FORMAT 1.00" 格式文件为 PNG 或原始格式
- 支持 V1 和 V2 版本的 ARC 文件格式

## 用法

```
arc-reader <file.arc> [path]
```

## LICENSE

保留了[原始项目的 LICENSE](./licence.txt)。
