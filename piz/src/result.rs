//! Error types and the related `Result<T>`

use thiserror::Error;

pub type ZipResult<T> = Result<T, ZipError>;

#[derive(Debug, Error)]
pub enum ZipError {
    /// An error from underlying I/O
    #[error("I/O Error")]
    Io(#[from] std::io::Error),

    /// The ZIP archive contained invalid data per the spec.
    #[error("Invalid Zip archive: {0}")]
    InvalidArchive(&'static str),

    /// Decoding a UTF-8 name or comment failed
    #[error("Invalid UTF-8")]
    Encoding(#[from] std::str::Utf8Error),

    /// The ZIP archive uses an unsupported feature
    #[error("Unsupported Zip archive: {0}")]
    UnsupportedArchive(String),

    /// The ZIP archive is prepended some unknown bytes.
    /// (Use [`ZipArchive::with_prepended_data()`] if this is okay.)
    ///
    /// [`ZipArchive::with_prepended_data()`]: ../read/struct.ZipArchive.html#method.with_prepended_data
    #[error("Archive prepended with {0} unknown bytes")]
    PrependedWithUnknownBytes(usize),

    /// A cast from a 64-bit int to a usize failed while mapping the file,
    /// probably on a 32-bit system.
    ///
    /// Future work could include a version of the reader that uses multiple
    /// file streams instead of a memory map to work with large files in 32 bits.
    #[error("Zip archive too large for address space")]
    InsufficientAddressSpace,
}
