use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArcError {
    #[error("IO错误: {0}")]
    Io(#[from] io::Error),

    #[error("无效的ARC文件格式")]
    InvalidFormat,

    #[error("文件索引越界: {0} >= {1}")]
    IndexOutOfBounds(u32, u32),

    #[error("无效的文件名编码: {0}")]
    InvalidFileName(#[from] std::str::Utf8Error),

    #[error("BSE解密失败")]
    BseDecryptError,

    #[error("DSC解密失败")]
    DscDecryptError,

    #[error("CBG解密失败")]
    CbgDecryptError,

    #[error("PNG文件处理失败")]
    PngProcessError,
}

pub type ArcResult<T> = Result<T, ArcError>;
