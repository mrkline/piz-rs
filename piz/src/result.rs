use thiserror::Error;

pub type ZipResult<T> = Result<T, ZipError>;

#[derive(Debug, Error)]
pub enum ZipError {
    #[error("I/O Error")]
    Io(#[from] std::io::Error),

    #[error("Invalid Zip archive: {0}")]
    InvalidArchive(&'static str),

    #[error("Invalid UTF-8")]
    Encoding(#[from] std::str::Utf8Error),

    #[error("Unsupported Zip archive: {0}")]
    UnsupportedArchive(String),

    #[error("Archive prepended with {0} unknown bytes")]
    PrependedWithUnknownBytes(usize),

    /// A cast from a 64-bit int to a usize failed while mapping the file,
    /// probably on a 32-bit system.
    ///
    /// Future work could include a version of the reader that uses multiple
    /// file streams instead of a memmap to work with large files in 32 bits.
    #[error("Zip archive too large for address space")]
    InsufficientAddressSpace,
}
