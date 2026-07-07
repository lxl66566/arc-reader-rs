use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use log::info;

use crate::error::{ArcError, ArcResult};

/// ARC archive format version.
///
/// V1 is the legacy `PackFile    ` format; V2 is the newer `BURIKO ARC20`
/// format. Each variant knows its own magic, per-entry metadata size and
/// filename field width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcVersion {
    V1,
    V2,
}

impl ArcVersion {
    /// Detect the version from a 12-byte magic signature.
    fn from_magic(magic: &[u8]) -> Option<Self> {
        match magic {
            b"PackFile    " => Some(Self::V1),
            b"BURIKO ARC20" => Some(Self::V2),
            _ => None,
        }
    }

    /// 12-byte magic signature used in the archive header.
    #[must_use]
    pub const fn magic(self) -> &'static [u8] {
        match self {
            Self::V1 => b"PackFile    ",
            Self::V2 => b"BURIKO ARC20",
        }
    }

    /// Per-entry metadata size in bytes.
    ///
    /// V1 entry: 16 (name) + 4 (offset) + 4 (size) + 8 (padding) = 32 bytes
    /// V2 entry: 96 (name) + 4 (offset) + 4 (size) + 24 (padding) = 128 bytes
    #[must_use]
    pub const fn metadata_size(self) -> u32 {
        match self {
            Self::V1 => 32,
            Self::V2 => 128,
        }
    }

    /// Filename field width in bytes.
    #[must_use]
    pub const fn name_len(self) -> usize {
        match self {
            Self::V1 => 16,
            Self::V2 => 96,
        }
    }
}

/// Represents a single file entry within an ARC archive.
#[derive(Debug, Clone)]
pub struct ArcFile {
    pub name: Vec<u8>,
    pub offset: u32,
    pub size: u32,
}

/// ARC archive reader.
pub struct Arc {
    file: File,
    data: u32,
    count: u32,
    version: ArcVersion,
    files: Vec<ArcFile>,
}

impl Arc {
    /// Open an ARC file and parse its index.
    pub fn open<P: AsRef<Path>>(filename: P) -> ArcResult<Self> {
        let mut file = File::open(&filename)?;

        // Read and validate the magic signature
        let mut magic_string = [0u8; 12];
        file.read_exact(&mut magic_string)?;

        let version = ArcVersion::from_magic(&magic_string).ok_or(ArcError::InvalidFormat)?;
        info!("ARC version: {version:?}");

        // Read the number of file entries
        let mut buffer = [0u8; 4];
        file.read_exact(&mut buffer)?;
        let number_of_files = u32::from_le_bytes(buffer);

        // Read all file metadata entries
        let mut files = Vec::with_capacity(number_of_files as usize);
        for _ in 0..number_of_files {
            let file_info = Self::read_metadata(&mut file, version)?;
            files.push(file_info);
        }

        let data_position = file.stream_position()? as u32;

        Ok(Arc {
            file,
            data: data_position,
            count: number_of_files,
            version,
            files,
        })
    }

    /// Returns the total number of files in the archive.
    #[must_use]
    pub fn files_count(&self) -> u32 {
        self.count
    }

    /// Returns the ARC format version.
    #[must_use]
    pub fn version(&self) -> ArcVersion {
        self.version
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
            u64::from(self.data) + u64::from(file_info.offset),
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

    /// Read a metadata entry.
    ///
    /// Layout: [name (`name_len`)][4 offset][4 size][trailing padding]
    fn read_metadata(file: &mut File, version: ArcVersion) -> ArcResult<ArcFile> {
        let mut name = vec![0u8; version.name_len()];
        file.read_exact(&mut name)?;

        sanitize_name(&mut name);

        let mut buffer = [0u8; 8];
        file.read_exact(&mut buffer)?;
        let offset = u32::from_le_bytes(buffer[0..4].try_into().unwrap());
        let size = u32::from_le_bytes(buffer[4..8].try_into().unwrap());

        // Skip trailing padding (V1: 8 bytes, V2: 24 bytes)
        let padding = i64::from(version.metadata_size()) - 8 - version.name_len() as i64;
        file.seek(SeekFrom::Current(padding))?;

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
