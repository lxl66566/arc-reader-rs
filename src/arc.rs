use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use log::info;

use crate::error::{ArcError, ArcResult};

// 定义版本特定的常量
pub const V1_MAGIC: &[u8] = b"PackFile    ";
pub const V2_MAGIC: &[u8] = b"BURIKO ARC20";
pub const V1_METADATA_SIZE: u32 = 32; // 16 (name) + 4 (offset) + 4 (size) + 8 (padding)
pub const V2_METADATA_SIZE: u32 = 112; // 16 (name) + 80 (20*4 padding) + 4 (offset) + 4 (size) + 24 (6*4 padding)

/// 文件结构体，表示 ARC 归档中的单个文件
#[derive(Debug, Clone)]
struct ArcFile {
    name: [u8; 16],
    offset: u32,
    size: u32,
}

/// ARC 归档结构体
pub struct Arc {
    file: File,
    data: u32,
    count: u32,
    files: Vec<ArcFile>,
}

impl Arc {
    /// 打开 ARC 文件并解析其内容
    pub fn open<P: AsRef<Path>>(filename: P) -> ArcResult<Self> {
        let mut file = File::open(&filename)?;

        // 检查是否为有效的 ARC 文件
        let mut magic_string = [0u8; 12];
        file.read_exact(&mut magic_string)?;

        let version = if &magic_string == b"PackFile    " {
            1 // v1
        } else if &magic_string == b"BURIKO ARC20" {
            2 // v2
        } else {
            return Err(ArcError::InvalidFormat);
        };
        info!("ARC 版本: {}", version);

        // 读取文件数量
        let mut buffer = [0u8; 4];
        file.read_exact(&mut buffer)?;
        let number_of_files = u32::from_le_bytes(buffer);

        // 读取文件元数据
        let mut files = Vec::with_capacity(number_of_files as usize);
        for _ in 0..number_of_files {
            let file_info = if version == 1 {
                Self::read_next_file_metadata_v1(&mut file)?
            } else {
                Self::read_next_file_metadata_v2(&mut file)?
            };
            files.push(file_info);
        }

        let data_position = file.stream_position()? as u32;

        Ok(Arc {
            file,
            data: data_position,
            count: number_of_files,
            files,
        })
    }

    /// 获取文件数量
    pub fn files_count(&self) -> u32 {
        self.count
    }

    /// 获取指定索引的文件数据
    pub fn get_file_data(&self, idx: u32) -> ArcResult<Vec<u8>> {
        if idx >= self.count {
            return Err(ArcError::IndexOutOfBounds(idx, self.count));
        }

        let file_info = &self.files[idx as usize];
        let mut data = vec![0u8; file_info.size as usize];

        let mut file_clone = self.file.try_clone()?;

        file_clone.seek(SeekFrom::Start((self.data + file_info.offset) as u64))?;

        file_clone.read_exact(&mut data)?;

        Ok(data)
    }

    /// 获取指定索引的文件大小
    pub fn get_file_size(&self, idx: u32) -> ArcResult<u32> {
        if idx >= self.count {
            return Err(ArcError::IndexOutOfBounds(idx, self.count));
        }
        Ok(self.files[idx as usize].size)
    }

    /// 获取指定索引的文件名
    pub fn get_file_name(&self, idx: u32) -> ArcResult<&str> {
        if idx >= self.count {
            return Err(ArcError::IndexOutOfBounds(idx, self.count));
        }

        let name_bytes = &self.files[idx as usize].name;
        // 找到第一个 0 作为字符串结束
        let len = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());

        // 转换为字符串
        Ok(std::str::from_utf8(&name_bytes[0..len])?)
    }

    // 读取 v1 版本的文件元数据
    fn read_next_file_metadata_v1(file: &mut File) -> ArcResult<ArcFile> {
        let mut name = [0u8; 16];
        file.read_exact(&mut name)?;

        // 清理非 ASCII 字节
        for j in 0..16 {
            if name[j] != 0 && (name[j] < 32 || name[j] > 127) {
                name[j] = b'_';
            }
        }

        let mut buffer = [0u8; 4];

        // 读取偏移量
        file.read_exact(&mut buffer)?;
        let offset = u32::from_le_bytes(buffer);

        // 读取大小
        file.read_exact(&mut buffer)?;
        let size = u32::from_le_bytes(buffer);

        // 跳过填充
        file.seek(SeekFrom::Current(8))?;

        Ok(ArcFile { name, offset, size })
    }

    // 读取 v2 版本的文件元数据
    fn read_next_file_metadata_v2(file: &mut File) -> ArcResult<ArcFile> {
        let mut name = [0u8; 16];
        file.read_exact(&mut name)?;

        // 清理非 ASCII 字节
        for j in 0..16 {
            if name[j] != 0 && (name[j] < 32 || name[j] > 127) {
                name[j] = b'_';
            }
        }

        // 跳过填充
        file.seek(SeekFrom::Current(20 * 4))?;

        let mut buffer = [0u8; 4];

        // 读取偏移量
        file.read_exact(&mut buffer)?;
        let offset = u32::from_le_bytes(buffer);

        // 读取大小
        file.read_exact(&mut buffer)?;
        let size = u32::from_le_bytes(buffer);

        // 跳过填充
        file.seek(SeekFrom::Current(6 * 4))?;

        Ok(ArcFile { name, offset, size })
    }
}
