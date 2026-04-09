use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use log::info;

use crate::error::{ArcError, ArcResult};

// Version-specific constants
pub const V1_MAGIC: &[u8] = b"PackFile    ";
pub const V2_MAGIC: &[u8] = b"BURIKO ARC20";

// V1 entry: 16 (name) + 4 (offset) + 4 (size) + 8 (padding) = 32 bytes
pub const V1_METADATA_SIZE: u32 = 32;
// V2 entry: 96 (name) + 4 (offset) + 4 (size) + 24 (padding) = 128 bytes (0x80)
pub const V2_METADATA_SIZE: u32 = 128;

pub const V1_NAME_LEN: usize = 16;
pub const V2_NAME_LEN: usize = 96;

/// Represents a single file entry within an ARC archive.
#[derive(Debug, Clone)]
struct ArcFile {
    name: Vec<u8>,
    offset: u32,
    size: u32,
}

/// ARC archive reader.
pub struct Arc {
    file: File,
    data: u32,
    count: u32,
    files: Vec<ArcFile>,
}

impl Arc {
    /// Open an ARC file and parse its index.
    pub fn open<P: AsRef<Path>>(filename: P) -> ArcResult<Self> {
        let mut file = File::open(&filename)?;

        // Read and validate the magic signature
        let mut magic_string = [0u8; 12];
        file.read_exact(&mut magic_string)?;

        let version = if &magic_string == b"PackFile    " {
            1
        } else if &magic_string == b"BURIKO ARC20" {
            2
        } else {
            return Err(ArcError::InvalidFormat);
        };
        info!("ARC version: {}", version);

        // Read the number of file entries
        let mut buffer = [0u8; 4];
        file.read_exact(&mut buffer)?;
        let number_of_files = u32::from_le_bytes(buffer);

        // Read all file metadata entries
        let mut files = Vec::with_capacity(number_of_files as usize);
        for _ in 0..number_of_files {
            let file_info = if version == 1 {
                Self::read_metadata_v1(&mut file)?
            } else {
                Self::read_metadata_v2(&mut file)?
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

    /// Returns the total number of files in the archive.
    pub fn files_count(&self) -> u32 {
        self.count
    }

    /// Read the raw data for the file at the given index.
    pub fn get_file_data(&self, idx: u32) -> ArcResult<Vec<u8>> {
        if idx >= self.count {
            return Err(ArcError::IndexOutOfBounds(idx, self.count));
        }

        let file_info = &self.files[idx as usize];
        let mut data = vec![0u8; file_info.size as usize];

        let mut file_clone = self.file.try_clone()?;

        file_clone.seek(SeekFrom::Start(
            (self.data as u64) + (file_info.offset as u64),
        ))?;

        file_clone.read_exact(&mut data)?;

        Ok(data)
    }

    /// Returns the original (compressed) size for the file at the given index.
    pub fn get_file_size(&self, idx: u32) -> ArcResult<u32> {
        if idx >= self.count {
            return Err(ArcError::IndexOutOfBounds(idx, self.count));
        }
        Ok(self.files[idx as usize].size)
    }

    /// Returns the null-terminated filename for the file at the given index.
    pub fn get_file_name(&self, idx: u32) -> ArcResult<&str> {
        if idx >= self.count {
            return Err(ArcError::IndexOutOfBounds(idx, self.count));
        }

        let name_bytes = &self.files[idx as usize].name;
        // Find the first null byte
        let len = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());

        Ok(std::str::from_utf8(&name_bytes[..len])?)
    }

    /// Read a V1 metadata entry (32 bytes per entry).
    ///
    /// Layout: [16 name][4 offset][4 size][8 padding]
    fn read_metadata_v1(file: &mut File) -> ArcResult<ArcFile> {
        let mut name = vec![0u8; V1_NAME_LEN];
        file.read_exact(&mut name)?;

        sanitize_name(&mut name);

        let mut buffer = [0u8; 8];
        file.read_exact(&mut buffer)?;
        let offset = u32::from_le_bytes(buffer[0..4].try_into().unwrap());
        let size = u32::from_le_bytes(buffer[4..8].try_into().unwrap());

        // Skip trailing padding
        file.seek(SeekFrom::Current(8))?;

        Ok(ArcFile { name, offset, size })
    }

    /// Read a V2 metadata entry (128 bytes per entry).
    ///
    /// Layout: [96 name][4 offset][4 size][24 padding]
    fn read_metadata_v2(file: &mut File) -> ArcResult<ArcFile> {
        let mut name = vec![0u8; V2_NAME_LEN];
        file.read_exact(&mut name)?;

        sanitize_name(&mut name);

        let mut buffer = [0u8; 8];
        file.read_exact(&mut buffer)?;
        let offset = u32::from_le_bytes(buffer[0..4].try_into().unwrap());
        let size = u32::from_le_bytes(buffer[4..8].try_into().unwrap());

        // Skip trailing 24 bytes padding (0x68..0x80)
        file.seek(SeekFrom::Current(24))?;

        Ok(ArcFile { name, offset, size })
    }
}

/// Replace non-printable-ASCII bytes in the name buffer with underscores.
fn sanitize_name(name: &mut [u8]) {
    for b in name.iter_mut() {
        if *b != 0 && (*b < 32 || *b > 127) {
            *b = b'_';
        }
    }
}
