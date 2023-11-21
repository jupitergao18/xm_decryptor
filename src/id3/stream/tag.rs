use crate::id3::storage::{PlainStorage, Storage, StorageFile};
use crate::id3::stream::{frame, unsynch};
use crate::id3::tag::{Tag, Version};
use crate::id3::taglike::TagLike;
use crate::id3::{Error, ErrorKind};
use bitflags::bitflags;
use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use std::cmp;
use std::fs;
use std::io::{self, Read, Write};
use std::ops::Range;
use std::path::Path;

static DEFAULT_FILE_DISCARD: &[&str] = &[
    "AENC", "ETCO", "EQUA", "MLLT", "POSS", "SYLT", "SYTC", "RVAD", "TENC", "TLEN", "TSIZ",
];

bitflags! {
    struct Flags: u8 {
        const UNSYNCHRONISATION = 0x80; // All versions
        const COMPRESSION       = 0x40; // =ID3v2.2
        const EXTENDED_HEADER   = 0x40; // >ID3v2.3, duplicate with TAG_COMPRESSION :(
        const EXPERIMENTAL      = 0x20; // >ID3v2.3
        const FOOTER            = 0x10; // >ID3v2.4
    }

    struct ExtFlags: u8 {
        const TAG_IS_UPDATE    = 0x40;
        const CRC_DATA_PRESENT = 0x20;
        const TAG_RESTRICTIONS = 0x10;
    }
}

/// Used for sharing code between sync/async parsers, which is mainly complicated by ext_headers.
struct HeaderBuilder {
    version: Version,
    flags: Flags,
    tag_size: u32,
}

impl HeaderBuilder {
    fn with_ext_header(self, size: u32) -> Header {
        Header {
            version: self.version,
            flags: self.flags,
            tag_size: self.tag_size,
            ext_header_size: size,
        }
    }
}

struct Header {
    version: Version,
    flags: Flags,
    tag_size: u32,

    // TODO: Extended header.
    ext_header_size: u32,
}

impl Header {
    fn size(&self) -> u64 {
        10 // Raw header.
    }

    fn frame_bytes(&self) -> u64 {
        u64::from(self.tag_size) - u64::from(self.ext_header_size)
    }

    fn tag_size(&self) -> u64 {
        self.size() + self.frame_bytes()
    }
}

impl Header {
    fn decode(mut reader: impl io::Read) -> crate::id3::Result<Header> {
        let mut header = [0; 10];
        let nread = reader.read(&mut header)?;
        let base_header = Self::decode_base_header(&header[..nread])?;

        // TODO: actually use the extended header data.
        let ext_header_size = if base_header.flags.contains(Flags::EXTENDED_HEADER) {
            let mut ext_header = [0; 6];
            reader.read_exact(&mut ext_header)?;
            let ext_size = unsynch::decode_u32(BigEndian::read_u32(&ext_header[0..4]));
            // The extended header size includes itself and always has at least 2 bytes following.
            if ext_size < 6 {
                return Err(Error::new(
                    ErrorKind::Parsing,
                    "Extended header requires has a minimum size of 6",
                ));
            }

            let _ext_flags = ExtFlags::from_bits_truncate(ext_header[5]);

            let ext_remaining_size = ext_size - ext_header.len() as u32;
            let mut ext_header = Vec::with_capacity(cmp::min(ext_remaining_size as usize, 0xffff));
            reader
                .by_ref()
                .take(ext_remaining_size as u64)
                .read_to_end(&mut ext_header)?;

            ext_size
        } else {
            0
        };

        Ok(base_header.with_ext_header(ext_header_size))
    }

    fn decode_base_header(header: &[u8]) -> crate::id3::Result<HeaderBuilder> {
        if header.len() != 10 {
            return Err(Error::new(
                ErrorKind::NoTag,
                "reader is not large enough to contain a id3 tag",
            ));
        }

        if &header[0..3] != b"ID3" {
            return Err(Error::new(
                ErrorKind::NoTag,
                "reader does not contain an id3 tag",
            ));
        }

        let (ver_major, ver_minor) = (header[3], header[4]);
        let version = match (ver_major, ver_minor) {
            (2, _) => Version::Id3v22,
            (3, _) => Version::Id3v23,
            (4, _) => Version::Id3v24,
            (_, _) => {
                return Err(Error::new(
                    ErrorKind::UnsupportedFeature,
                    format!(
                        "Unsupported id3 tag version: v2.{}.{}",
                        ver_major, ver_minor
                    ),
                ));
            }
        };
        let flags = Flags::from_bits(header[5])
            .ok_or_else(|| Error::new(ErrorKind::Parsing, "unknown tag header flags are set"))?;
        let tag_size = unsynch::decode_u32(BigEndian::read_u32(&header[6..10]));

        // compression only exists on 2.2 and conflicts with 2.3+'s extended header
        if version == Version::Id3v22 && flags.contains(Flags::COMPRESSION) {
            return Err(Error::new(
                ErrorKind::UnsupportedFeature,
                "id3v2.2 compression is not supported",
            ));
        }

        Ok(HeaderBuilder {
            version,
            flags,
            tag_size,
        })
    }
}

pub fn decode(mut reader: impl io::Read) -> crate::id3::Result<Tag> {
    let header = Header::decode(&mut reader)?;

    decode_remaining(reader, header)
}

fn decode_remaining(mut reader: impl io::Read, header: Header) -> crate::id3::Result<Tag> {
    match header.version {
        Version::Id3v22 => {
            // Limit the reader only to the given tag_size, don't return any more bytes after that.
            let v2_reader = reader.take(header.frame_bytes());

            if header.flags.contains(Flags::UNSYNCHRONISATION) {
                // Unwrap all 'unsynchronized' bytes in the tag before parsing frames.
                decode_v2_frames(unsynch::Reader::new(v2_reader))
            } else {
                decode_v2_frames(v2_reader)
            }
        }
        Version::Id3v23 => {
            // Unsynchronization is applied to the whole tag, excluding the header.
            let mut reader: Box<dyn io::Read> = if header.flags.contains(Flags::UNSYNCHRONISATION) {
                Box::new(unsynch::Reader::new(reader))
            } else {
                Box::new(reader)
            };

            let mut offset = 0;
            let mut tag = Tag::with_version_tag_size(header.version, header.tag_size());
            while offset < header.frame_bytes() {
                let v = match frame::v3::decode(&mut reader) {
                    Ok(v) => v,
                    Err(err) => return Err(err.with_tag(tag)),
                };
                let (bytes_read, frame) = match v {
                    Some(v) => v,
                    None => break, // Padding.
                };
                tag.add_frame(frame);
                offset += bytes_read as u64;
            }
            Ok(tag)
        }
        Version::Id3v24 => {
            let mut offset = 0;
            let mut tag = Tag::with_version(header.version);

            while offset < header.frame_bytes() {
                let v = match frame::v4::decode(&mut reader) {
                    Ok(v) => v,
                    Err(err) => return Err(err.with_tag(tag)),
                };
                let (bytes_read, frame) = match v {
                    Some(v) => v,
                    None => break, // Padding.
                };
                tag.add_frame(frame);
                offset += bytes_read as u64;
            }
            Ok(tag)
        }
    }
}

pub fn decode_v2_frames(mut reader: impl io::Read) -> crate::id3::Result<Tag> {
    let mut tag = Tag::with_version(Version::Id3v22);
    // Add all frames, until either an error is thrown or there are no more frames to parse
    // (because of EOF or a Padding).
    loop {
        let v = match frame::v2::decode(&mut reader) {
            Ok(v) => v,
            Err(err) => return Err(err.with_tag(tag)),
        };
        match v {
            Some((_bytes_read, frame)) => {
                tag.add_frame(frame);
            }
            None => break Ok(tag),
        }
    }
}

/// The `Encoder` may be used to encode tags with custom settings.
#[derive(Clone, Debug)]
pub struct Encoder {
    version: Version,
    unsynchronisation: bool,
    compression: bool,
    file_altered: bool,
    padding: Option<usize>,
}

impl Encoder {
    /// Constructs a new `Encoder` with the following configuration:
    ///
    /// * [`Version`] is ID3v2.4
    /// * Unsynchronization is disabled due to compatibility issues
    /// * No compression
    /// * File is not marked as altered
    pub fn new() -> Self {
        Self {
            version: Version::Id3v24,
            unsynchronisation: false,
            compression: false,
            file_altered: false,
            padding: None,
        }
    }

    /// Sets the padding that is written after the tag.
    ///
    /// Should be only used when writing to a MP3 file
    pub fn padding(mut self, padding: usize) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Sets the ID3 version.
    pub fn version(mut self, version: Version) -> Self {
        self.version = version;
        self
    }

    /// Enables or disables the unsynchronisation scheme.
    ///
    /// This avoids patterns that resemble MP3-frame headers from being
    /// encoded. If you are encoding to MP3 files and wish to be compatible
    /// with very old tools, you probably want this enabled.
    pub fn unsynchronisation(mut self, unsynchronisation: bool) -> Self {
        self.unsynchronisation = unsynchronisation;
        self
    }

    /// Enables or disables compression.
    pub fn compression(mut self, compression: bool) -> Self {
        self.compression = compression;
        self
    }

    /// Informs the encoder whether the file this tag belongs to has been changed.
    ///
    /// This subsequently discards any tags that have their File Alter Preservation bits set and
    /// that have a relation to the file contents:
    ///
    ///   AENC, ETCO, EQUA, MLLT, POSS, SYLT, SYTC, RVAD, TENC, TLEN, TSIZ
    pub fn file_altered(mut self, file_altered: bool) -> Self {
        self.file_altered = file_altered;
        self
    }

    /// Encodes the specified [`Tag`] using the settings set in the [`Encoder`].
    ///
    /// Note that the plain tag is written, regardless of the original contents. To safely encode a
    /// tag to an MP3 file, use [`Encoder::encode_to_path`].
    pub fn encode(&self, tag: &Tag, mut writer: impl io::Write) -> crate::id3::Result<()> {
        // remove frames which have the flags indicating they should be removed
        let saved_frames = tag
            .frames()
            // Assert that by encoding, we are changing the tag. If the Tag Alter Preservation bit
            // is set, discard the frame.
            .filter(|frame| !frame.tag_alter_preservation())
            // If the file this tag belongs to is updated, check for the File Alter Preservation
            // bit.
            .filter(|frame| !self.file_altered || !frame.file_alter_preservation())
            // Check whether this frame is part of the set of frames that should always be
            // discarded when the file is changed.
            .filter(|frame| !self.file_altered || !DEFAULT_FILE_DISCARD.contains(&frame.id()));

        let mut flags = Flags::empty();
        flags.set(Flags::UNSYNCHRONISATION, self.unsynchronisation);
        if self.version == Version::Id3v22 {
            flags.set(Flags::COMPRESSION, self.compression);
        }

        let mut frame_data = Vec::new();
        for frame in saved_frames {
            frame.validate()?;
            frame::encode(&mut frame_data, frame, self.version, self.unsynchronisation)?;
        }
        // In ID3v2.2/ID3v2.3, Unsynchronization is applied to the whole tag data at once, not for
        // each frame separately.
        if self.unsynchronisation {
            match self.version {
                Version::Id3v22 | Version::Id3v23 => unsynch::encode_vec(&mut frame_data),
                Version::Id3v24 => {}
            };
        }
        let tag_size = frame_data.len() + self.padding.unwrap_or(0);
        writer.write_all(b"ID3")?;
        writer.write_all(&[self.version.minor(), 0])?;
        writer.write_u8(flags.bits())?;
        writer.write_u32::<BigEndian>(unsynch::encode_u32(tag_size as u32))?;
        writer.write_all(&frame_data[..])?;

        if let Some(padding) = self.padding {
            writer.write_all(&vec![0; padding])?;
        }
        Ok(())
    }

    /// Encodes a [`Tag`] and replaces any existing tag in the file.
    pub fn write_to_file(&self, tag: &Tag, mut file: impl StorageFile) -> crate::id3::Result<()> {
        #[allow(clippy::reversed_empty_ranges)]
        let location = locate_id3v2(&mut file)?.unwrap_or(0..0); // Create a new tag if none could be located.

        let mut storage = PlainStorage::new(file, location);
        let mut w = storage.writer()?;
        self.encode(tag, &mut w)?;
        w.flush()?;
        Ok(())
    }

    /// Encodes a [`Tag`] and replaces any existing tag in the file.
    #[deprecated(note = "Use write_to_file")]
    pub fn encode_to_file(&self, tag: &Tag, file: &mut fs::File) -> crate::id3::Result<()> {
        self.write_to_file(tag, file)
    }

    /// Encodes a [`Tag`] and replaces any existing tag in the file pointed to by the specified path.
    pub fn write_to_path(&self, tag: &Tag, path: impl AsRef<Path>) -> crate::id3::Result<()> {
        let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;
        self.write_to_file(tag, &mut file)?;
        file.flush()?;
        Ok(())
    }

    /// Encodes a [`Tag`] and replaces any existing tag in the file pointed to by the specified path.
    #[deprecated(note = "Use write_to_path")]
    pub fn encode_to_path(&self, tag: &Tag, path: impl AsRef<Path>) -> crate::id3::Result<()> {
        self.write_to_path(tag, path)
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn locate_id3v2(
    mut reader: impl io::Read + io::Seek,
) -> crate::id3::Result<Option<Range<u64>>> {
    let header = match Header::decode(&mut reader) {
        Ok(v) => v,
        Err(err) => match err.kind {
            ErrorKind::NoTag => return Ok(None),
            _ => return Err(err),
        },
    };

    let tag_size = header.tag_size();
    reader.seek(io::SeekFrom::Start(tag_size))?;
    let num_padding = reader
        .bytes()
        .take_while(|rs| rs.as_ref().map(|b| *b == 0x00).unwrap_or(false))
        .count();
    Ok(Some(0..tag_size + num_padding as u64))
}
