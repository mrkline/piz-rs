use thiserror::Error;

pub type ZipResult<T> = Result<T, ZipError>;

#[derive(Debug, Error)]
pub enum ZipError {
    #[error("I/O Error")]
    Io(#[from] std::io::Error),

    #[error("Invalid Zip archive: {0}")]
    InvalidArchive(&'static str),
}
