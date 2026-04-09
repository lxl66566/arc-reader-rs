# ARC Format 开发文档：Rust 实现与 GARBro 差异分析

## 1. ARC 归档格式差异

### 1.1 V2 文件名长度

| 项目 | Rust (当前) | GARBro |
|------|------------|--------|
| V2 文件名长度 | 16 字节 | **96 字节 (0x60)** |
| V2 条目总大小 | 112 字节 | 128 字节 (0x80) |
| V2 偏移量位置 | 16 + 80 = offset 0x60 | offset 0x60 ✓ |
| V2 大小位置 | offset 0x64 | offset 0x64 ✓ |

**关键发现**: GARBro 的 `Arc2Opener.TryOpen()` 读取 V2 条目时：
```csharp
string name = file.View.ReadString(index_offset, 0x60);  // 96 字节文件名
var offset = base_offset + file.View.ReadUInt32(index_offset+0x60);
entry.Size = file.View.ReadUInt32(index_offset+0x64);
index_offset += 0x80;  // 每个条目 128 字节
```

Rust 代码只读 16 字节文件名，然后跳过 80 字节填充。偏移量和大小字段的位置碰巧是对的，但文件名被截断为 16 字节，导致新游戏中较长的文件名无法正确读取。

### 1.2 V1 格式

两者一致：每个条目 0x20 (32) 字节，16 字节文件名。

## 2. CBG (CompressedBG) 格式差异

### 2.1 版本检测

GARBro 通过头部 offset 0x2E 处的 `Version` 字段（u16）区分版本：
- **Version < 2**: V1 解码（Huffman + 零行程 + 平均采样逆变换）
- **Version == 2**: V2 解码（DCT 变换 + Huffman + YCbCr→RGB）
- **Version > 2**: 不支持

Rust 代码目前**完全忽略** Version 字段，只实现了 V1 解码。

### 2.2 头部布局

```
Offset  Size  Description
0x00    16    Magic: "CompressedBG___"
0x10    2     Width (u16 LE)
0x12    2     Height (u16 LE)
0x14    4     BPP (i32 LE) - 支持 8/16/24/32
0x18    4     (unknown)
0x1C    4     (unknown)
0x20    4     IntermediateLength (i32 LE) - V1 中间缓冲区长度
0x24    4     Key (u32 LE) - 解密密钥
0x28    4     EncLength (i32 LE) - 加密数据长度
0x2C    1     CheckSum
0x2D    1     CheckXor
0x2E    2     Version (u16 LE) - 版本号
```

Rust 代码正确读取了前 48 字节头部，但**未读取 Version 字段**。

### 2.3 V1 解码流程

GARBro 和 Rust 的 V1 解码流程基本一致：
1. 读取加密数据 → 用 key stream 解密 → 校验 sum/xor
2. 从解密数据构建 256 项权重表
3. 构建 Huffman 树
4. Huffman 解压缩到中间缓冲区
5. UnpackZeros（零行程解码）
6. ReverseAverageSampling（逆向平均采样）

**差异**: GARBro 的平均采样逆变换是按像素分量（byte-by-byte）操作的，而 Rust 代码按 32 位颜色值操作。对于 BPP=24 的情况，GARBro 的方法更正确。

### 2.4 V2 解码流程（仅 GARBro 支持）

V2 使用完全不同的解码算法：

1. **DCT 系数表**: 从加密数据（0x80 字节）× DCT_Table 初始化
2. **两个 Huffman 树**: Tree1 (0x10 项) 和 Tree2 (0xB0 项)
3. **分块处理**: 图像按 8×8 块处理，每行块可并行解码
4. **行偏移表**: 每行 8 像素块有一个偏移量
5. **块解码过程**:
   - DC 系数：Tree1 解码 + 累加
   - AC 系数：Tree2 解码 + zigzag 扫描顺序
   - IDCT 变换
   - YCbCr → RGB 转换
6. **Alpha 通道**: 可选，32bpp 时存在独立 alpha 解码

### 2.5 V2 Huffman 树构建

GARBro 的 HuffmanTree 类对 V2 使用 `v2=true` 模式：
- 在查找最小权重节点时，第一个子节点使用**第一个找到的有效节点**（而非最小权重节点）
- 仅对第二个子节点使用最小权重搜索
- 这确保了树的结构与编码端一致

Rust 代码的 Huffman 树构建（`method2` 函数）是 V1 专用的，不兼容 V2。

### 2.6 BPP 16 支持

GARBro 支持 Bgr565 格式（16bpp），但仅限 V1。V2 不支持 16bpp。

## 3. BSE 加密差异

### 3.1 版本支持

| 版本 | Rust | GARBro |
|------|------|--------|
| BSE 1.0 | ✓ | ✓ |
| BSE 1.1 | ✗ | ✓ |

GARBro 根据 offset 0x08 处的版本号区分：
- `0x100`: BSE 1.0（使用 BseGenerator100）
- `0x101`: BSE 1.1（使用 BseGenerator101）

### 3.2 密钥生成算法

**BSE 1.0 (相同)**:
```c
v = ((key * 257 >> 8) + key * 97 + 23) ^ 0xA6CD9B75
key = RotR(v, 16)
```

**BSE 1.1 (仅 GARBro)**:
```c
v = ((key * 127 >> 7) + key * 83 + 53) ^ 0xB97A7E5C
key = RotR(v, 16)
```

### 3.3 解密方式差异

- **Rust**: 直接在原数据上修改，处理前 64 字节
- **GARBro**: 读取 header 副本进行解密，body 部分从 offset 0x50 开始读取
  - 即 header = 0x10~0x4F（前 0x40 字节加密数据）
  - body = 0x50 之后的数据（不加密）
  - 最终输出 = 解密后的 header + body

GARBro 的 BSE 处理更精确：
1. 从 offset 0x10 读取 0x40 字节 header
2. 根据 BSE 版本选择密钥生成器解密 header
3. body 从 offset 0x50 开始，不加密
4. 输出 = 解密后的 header + body

Rust 代码直接跳过前 16 字节然后处理，这在 BSE 1.0 场景下基本正确，但不够精确。

## 4. DSC 格式差异

### 4.1 Huffman 树构建

- **Rust**: 使用 buffer 排序 + 逐层构建方式
- **GARBro**: 使用 HuffmanCode 排序 + 逐层构建方式

两者实现逻辑不同但最终效果等价。GARBro 的实现更清晰。

### 4.2 Huffman 解压缩

- **Rust**: 使用自定义 bit 读取（`mask` 变量）
- **GARBro**: 使用 `MsbBitStream` 基类（MSB 优先的位流）

两者最终效果等价。

## 5. BGI 简单图片格式

Rust 代码通过 DSC 解码后间接支持部分 BGI 图片。GARBro 有独立的 `BgiFormat`：
- 支持 8/24/32 bpp
- 支持 flag=1 时的像素加扰（蛇形扫描 + 增量编码）
- 16 字节头部：width(i16) + height(i16) + bpp(i16) + flag(i16) + 8 零字节

Rust 代码的 `dsc_is_image()` 和 `dsc::save()` 实现了类似功能，但不支持 flag=1 的加扰模式。

## 6. 音频格式差异

| 特性 | Rust | GARBro |
|------|------|--------|
| OGG 检测 | offset 64 处 "OggS" | offset 4 处 "bw  " |
| OGG 偏移 | 固定跳过 64 字节 | 读取前 4 字节作为偏移量 |

GARBro 读取文件前 4 字节（u32 LE）作为 Ogg 数据的偏移量，而非固定 64 字节。Rust 代码的固定 64 字节跳过可能在某些文件上不正确。

## 7. 需要修改的优先级

### 高优先级
1. **V2 文件名扩展到 96 字节** - 直接影响文件名读取
2. **CBG V2 解码** - 素晴日 15 周年版等新游戏图片需要

### 中优先级
3. **BSE 1.1 支持** - 部分新游戏可能使用
4. **OGG 偏移量动态读取** - 更精确的音频处理

### 低优先级
5. **BGI 图片加扰模式支持** - flag=1 的特殊处理
6. **DSC Huffman 重构** - 提高代码清晰度
