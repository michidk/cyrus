use std::fmt;
use std::io;

#[derive(Debug)]
pub enum SmkError {
    Io(io::Error),
    InvalidSignature,
    BitstreamExhausted,
    TreeBuildFailed(&'static str),
    InvalidData(&'static str),
}

impl fmt::Display for SmkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SmkError::Io(e) => write!(f, "I/O error: {e}"),
            SmkError::InvalidSignature => write!(f, "invalid SMK signature"),
            SmkError::BitstreamExhausted => write!(f, "bitstream exhausted"),
            SmkError::TreeBuildFailed(msg) => write!(f, "huffman tree build failed: {msg}"),
            SmkError::InvalidData(msg) => write!(f, "invalid data: {msg}"),
        }
    }
}

impl std::error::Error for SmkError {}

impl From<io::Error> for SmkError {
    fn from(e: io::Error) -> Self {
        SmkError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, SmkError>;
