pub use error::{partial_tag_ok, Error, ErrorKind, Result};
pub use frame::{Content, Frame, Timestamp};
pub use storage::StorageFile;
pub use stream::encoding::Encoding;
pub use stream::tag::Encoder;
pub use tag::{Tag, Version};
pub use taglike::TagLike;

/// Contains types and methods for operating on ID3 frames.
pub mod frame;
/// Utilities for working with ID3v1 tags.
pub mod v1;
/// Combined API that handles both ID3v1 and ID3v2 tags at the same time.
pub mod v1v2;

mod chunk;
mod error;
mod storage;
mod stream;
mod tag;
mod taglike;
mod tcon;
