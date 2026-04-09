use std::io;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArcError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid ARC file format")]
    InvalidFormat,

    #[error("File index out of bounds: {0} >= {1}")]
    IndexOutOfBounds(u32, u32),

    #[error("Invalid filename encoding: {0}")]
    InvalidFileName(#[from] std::str::Utf8Error),

    #[error("BSE decryption failed")]
    BseDecryptError,

    #[error("DSC decryption failed")]
    DscDecryptError,

    #[error("CBG decryption failed")]
    CbgDecryptError,

    #[error("Unsupported CBG version: {0}")]
    CbgUnsupportedVersion(u16),

    #[error("PNG encoding failed")]
    PngProcessError,

    #[error("Unsupported file type: {0}")]
    UnsupportedFileType(String),
}

pub type ArcResult<T> = Result<T, ArcError>;
