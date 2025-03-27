use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

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
    pub fn open<P: AsRef<Path>>(filename: P) -> Option<Self> {
        let mut file = match File::open(filename) {
            Ok(f) => f,
            Err(_) => return None,
        };

        // 检查是否为有效的 ARC 文件
        let mut magic_string = [0u8; 12];
        if file.read_exact(&mut magic_string).is_err() {
            return None;
        }

        let version = if &magic_string == b"PackFile    " {
            1 // v1
        } else if &magic_string == b"BURIKO ARC20" {
            2 // v2
        } else {
            return None;
        };

        // 读取文件数量
        let mut buffer = [0u8; 4];
        if file.read_exact(&mut buffer).is_err() {
            return None;
        }
        let number_of_files = u32::from_le_bytes(buffer);

        // 读取文件元数据
        let mut files = Vec::with_capacity(number_of_files as usize);
        for _ in 0..number_of_files {
            let file_info = if version == 1 {
                Self::read_next_file_metadata_v1(&mut file)
            } else {
                Self::read_next_file_metadata_v2(&mut file)
            };

            if let Some(f) = file_info {
                files.push(f);
            } else {
                return None;
            }
        }

        let data_position = match file.stream_position() {
            Ok(pos) => pos as u32,
            Err(_) => return None,
        };

        Some(Arc {
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
    pub fn get_file_data(&self, idx: u32) -> Option<Vec<u8>> {
        if idx >= self.count {
            return None;
        }

        let file_info = &self.files[idx as usize];
        let mut data = vec![0u8; file_info.size as usize];

        let mut file_clone = self.file.try_clone().ok()?;

        if file_clone
            .seek(SeekFrom::Start((self.data + file_info.offset) as u64))
            .is_err()
        {
            return None;
        }

        if file_clone.read_exact(&mut data).is_err() {
            return None;
        }

        Some(data)
    }

    /// 获取指定索引的文件大小
    pub fn get_file_size(&self, idx: u32) -> u32 {
        if idx >= self.count {
            return 0;
        }
        self.files[idx as usize].size
    }

    /// 获取指定索引的文件名
    pub fn get_file_name(&self, idx: u32) -> &str {
        if idx >= self.count {
            return "";
        }

        let name_bytes = &self.files[idx as usize].name;
        // 找到第一个 0 作为字符串结束
        let len = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());

        // 转换为字符串，忽略无效的 UTF-8 序列
        std::str::from_utf8(&name_bytes[0..len]).unwrap_or("")
    }

    // 读取 v1 版本的文件元数据
    fn read_next_file_metadata_v1(file: &mut File) -> Option<ArcFile> {
        let mut name = [0u8; 16];
        if file.read_exact(&mut name).is_err() {
            return None;
        }

        // 清理非 ASCII 字节
        for j in 0..16 {
            if name[j] != 0 && (name[j] < 32 || name[j] > 127) {
                name[j] = b'_';
            }
        }

        let mut buffer = [0u8; 4];

        // 读取偏移量
        if file.read_exact(&mut buffer).is_err() {
            return None;
        }
        let offset = u32::from_le_bytes(buffer);

        // 读取大小
        if file.read_exact(&mut buffer).is_err() {
            return None;
        }
        let size = u32::from_le_bytes(buffer);

        // 跳过填充
        if file.seek(SeekFrom::Current(8)).is_err() {
            return None;
        }

        Some(ArcFile { name, offset, size })
    }

    // 读取 v2 版本的文件元数据
    fn read_next_file_metadata_v2(file: &mut File) -> Option<ArcFile> {
        let mut name = [0u8; 16];
        if file.read_exact(&mut name).is_err() {
            return None;
        }

        // 清理非 ASCII 字节
        for j in 0..16 {
            if name[j] != 0 && (name[j] < 32 || name[j] > 127) {
                name[j] = b'_';
            }
        }

        // 跳过填充
        if file.seek(SeekFrom::Current(20 * 4)).is_err() {
            return None;
        }

        let mut buffer = [0u8; 4];

        // 读取偏移量
        if file.read_exact(&mut buffer).is_err() {
            return None;
        }
        let offset = u32::from_le_bytes(buffer);

        // 读取大小
        if file.read_exact(&mut buffer).is_err() {
            return None;
        }
        let size = u32::from_le_bytes(buffer);

        // 跳过填充
        if file.seek(SeekFrom::Current(6 * 4)).is_err() {
            return None;
        }

        Some(ArcFile { name, offset, size })
    }
}

// 辅助函数，从文件中读取 u32 值
fn _read_u32_from_file(file: &mut File) -> io::Result<u32> {
    let mut buffer = [0u8; 4];
    file.read_exact(&mut buffer)?;
    Ok(u32::from_le_bytes(buffer))
}
