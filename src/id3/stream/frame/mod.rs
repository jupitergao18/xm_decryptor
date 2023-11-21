use crate::id3::frame::Content;
use crate::id3::frame::Frame;
use crate::id3::stream::encoding::Encoding;
use crate::id3::stream::unsynch;
use crate::id3::tag::Version;
use flate2::read::ZlibDecoder;
use std::io;
use std::str;

pub mod content;
pub mod v2;
pub mod v3;
pub mod v4;

pub fn decode(
    reader: impl io::Read,
    version: Version,
) -> crate::id3::Result<Option<(usize, Frame)>> {
    match version {
        Version::Id3v22 => unimplemented!(),
        Version::Id3v23 => v3::decode(reader),
        Version::Id3v24 => v4::decode(reader),
    }
}

fn decode_content(
    reader: impl io::Read,
    version: Version,
    id: &str,
    compression: bool,
    unsynchronisation: bool,
) -> crate::id3::Result<(Content, Option<Encoding>)> {
    if unsynchronisation {
        let reader_unsynch = unsynch::Reader::new(reader);
        if compression {
            content::decode(id, version, ZlibDecoder::new(reader_unsynch))
        } else {
            content::decode(id, version, reader_unsynch)
        }
    } else if compression {
        content::decode(id, version, ZlibDecoder::new(reader))
    } else {
        content::decode(id, version, reader)
    }
}

pub fn encode(
    writer: impl io::Write,
    frame: &Frame,
    version: Version,
    unsynchronization: bool,
) -> crate::id3::Result<usize> {
    match version {
        Version::Id3v22 => v2::encode(writer, frame),
        Version::Id3v23 => {
            let mut flags = v3::Flags::empty();
            flags.set(
                v3::Flags::TAG_ALTER_PRESERVATION,
                frame.tag_alter_preservation(),
            );
            flags.set(
                v3::Flags::FILE_ALTER_PRESERVATION,
                frame.file_alter_preservation(),
            );
            v3::encode(writer, frame, flags)
        }
        Version::Id3v24 => {
            let mut flags = v4::Flags::empty();
            flags.set(v4::Flags::UNSYNCHRONISATION, unsynchronization);
            flags.set(
                v4::Flags::TAG_ALTER_PRESERVATION,
                frame.tag_alter_preservation(),
            );
            flags.set(
                v4::Flags::FILE_ALTER_PRESERVATION,
                frame.file_alter_preservation(),
            );
            v4::encode(writer, frame, flags)
        }
    }
}

/// Helper for str::from_utf8 that preserves any problematic pattern if applicable.
pub fn str_from_utf8(b: &[u8]) -> crate::id3::Result<&str> {
    str::from_utf8(b).map_err(|err| {
        let bad = b[err.valid_up_to()..].to_vec();
        crate::id3::Error {
            kind: crate::id3::ErrorKind::StringDecoding(bad.to_vec()),
            description: "data is not valid utf-8".to_string(),
            partial_tag: None,
        }
    })
}
