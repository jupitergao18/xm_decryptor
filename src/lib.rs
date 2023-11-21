pub mod id3;
pub mod xm;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
