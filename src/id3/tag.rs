use crate::id3::chunk;
use crate::id3::frame::{
    Chapter, Comment, EncapsulatedObject, ExtendedLink, ExtendedText, Frame, Lyrics, Picture,
    SynchronisedLyrics, TableOfContents,
};
use crate::id3::storage::{PlainStorage, Storage};
use crate::id3::stream;
use crate::id3::taglike::TagLike;
use crate::id3::v1;
use crate::id3::StorageFile;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::iter::{FromIterator, Iterator};
use std::path::Path;

/// Denotes the version of a tag.
#[derive(Copy, Clone, Default, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Version {
    /// ID3v2.2
    Id3v22,
    /// ID3v2.3
    Id3v23,
    /// ID3v2.4
    #[default]
    Id3v24,
}

impl Version {
    /// Returns the minor version.
    ///
    /// # Example
    /// ```
    /// use id3::Version;
    ///
    /// assert_eq!(Version::Id3v24.minor(), 4);
    /// ```
    pub fn minor(self) -> u8 {
        match self {
            Version::Id3v22 => 2,
            Version::Id3v23 => 3,
            Version::Id3v24 => 4,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Version::Id3v22 => write!(f, "ID3v2.2"),
            Version::Id3v23 => write!(f, "ID3v2.3"),
            Version::Id3v24 => write!(f, "ID3v2.4"),
        }
    }
}

/// An ID3 tag containing zero or more [`Frame`]s.
#[derive(Clone, Debug, Default, Eq)]
pub struct Tag {
    /// A vector of frames included in the tag.
    frames: Vec<Frame>,
    /// ID3 Tag version
    version: Version,
    header_tag_size: u64,
}

impl<'a> Tag {
    /// Creates a new ID3v2.4 tag with no frames.
    pub fn new() -> Tag {
        Tag::default()
    }

    /// Used for creating new tag with a specific version.
    pub fn with_version(version: Version) -> Tag {
        Tag {
            version,
            ..Tag::default()
        }
    }

    /// Used for creating new tag with a specific version and header tag size.
    pub fn with_version_tag_size(version: Version, header_tag_size: u64) -> Tag {
        Tag {
            version,
            header_tag_size,
            ..Tag::default()
        }
    }

    // Read/write functions are declared below. We adhere to the following naming conventions:
    // * <format> -> io::Read/io::Write (+ io::Seek?)
    // * <format>_path -> impl AsRef<Path>
    // * <format>_file -> &mut File

    /// Will return true if the reader is a candidate for an ID3 tag. The reader position will be
    /// reset back to the previous position before returning.
    pub fn is_candidate(mut reader: impl io::Read + io::Seek) -> crate::id3::Result<bool> {
        let initial_position = reader.stream_position()?;
        let rs = stream::tag::locate_id3v2(&mut reader);
        reader.seek(io::SeekFrom::Start(initial_position))?;
        Ok(rs?.is_some())
    }

    /// Detects the presence of an ID3v2 tag at the current position of the reader and skips it
    /// if is found. Returns true if a tag was found.
    pub fn skip(mut reader: impl io::Read + io::Seek) -> crate::id3::Result<bool> {
        let initial_position = reader.stream_position()?;
        let range = stream::tag::locate_id3v2(&mut reader)?;
        let end = range.as_ref().map(|r| r.end).unwrap_or(0);
        reader.seek(io::SeekFrom::Start(initial_position + end))?;
        Ok(range.is_some())
    }

    /// Removes an ID3v2 tag from the file at the specified path.
    ///
    /// Returns true if the file initially contained a tag.
    pub fn remove_from_path(path: impl AsRef<Path>) -> crate::id3::Result<bool> {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .truncate(false)
            .open(path)?;
        Self::remove_from_file(&mut file)
    }

    /// Removes an ID3v2 tag from the specified file.
    ///
    /// Returns true if the file initially contained a tag.
    pub fn remove_from_file(mut file: &mut fs::File) -> crate::id3::Result<bool> {
        let location = match stream::tag::locate_id3v2(&mut file)? {
            Some(l) => l,
            None => return Ok(false),
        };
        // Open the ID3 region for writing and write nothing. This removes the region in its
        // entirety.
        let mut storage = PlainStorage::new(file, location);
        storage.writer()?.flush()?;
        Ok(true)
    }

    /// Attempts to read an ID3 tag from the reader.
    pub fn read_from(reader: impl io::Read) -> crate::id3::Result<Tag> {
        stream::tag::decode(reader)
    }

    /// Attempts to read an ID3 tag via Tokio from the reader.
    #[cfg(feature = "tokio")]
    pub async fn async_read_from(
        reader: impl tokio::io::AsyncRead + std::marker::Unpin,
    ) -> crate::id3::Result<Tag> {
        stream::tag::async_decode(reader).await
    }

    /// Attempts to read an ID3 tag from the file at the indicated path.
    pub fn read_from_path(path: impl AsRef<Path>) -> crate::id3::Result<Tag> {
        let file = BufReader::new(File::open(path)?);
        Tag::read_from(file)
    }

    /// Attempts to read an ID3 tag via Tokio from the file at the indicated path.
    #[cfg(feature = "tokio")]
    pub async fn async_read_from_path(path: impl AsRef<Path>) -> crate::id3::Result<Tag> {
        let file = tokio::io::BufReader::new(tokio::fs::File::open(path).await?);
        stream::tag::async_decode(file).await
    }

    /// Reads an AIFF stream and returns any present ID3 tag.
    pub fn read_from_aiff(reader: impl io::Read + io::Seek) -> crate::id3::Result<Tag> {
        chunk::load_id3_chunk::<chunk::AiffFormat, _>(reader)
    }

    /// Reads an AIFF file at the specified path and returns any present ID3 tag.
    pub fn read_from_aiff_path(path: impl AsRef<Path>) -> crate::id3::Result<Tag> {
        let mut file = BufReader::new(File::open(path)?);
        chunk::load_id3_chunk::<chunk::AiffFormat, _>(&mut file)
    }

    /// Reads an AIFF file and returns any present ID3 tag.
    pub fn read_from_aiff_file(file: &mut fs::File) -> crate::id3::Result<Tag> {
        chunk::load_id3_chunk::<chunk::AiffFormat, _>(file)
    }

    /// Reads an WAV stream and returns any present ID3 tag.
    pub fn read_from_wav(reader: impl io::Read + io::Seek) -> crate::id3::Result<Tag> {
        chunk::load_id3_chunk::<chunk::WavFormat, _>(reader)
    }

    /// Reads an WAV file at the specified path and returns any present ID3 tag.
    pub fn read_from_wav_path(path: impl AsRef<Path>) -> crate::id3::Result<Tag> {
        let mut file = BufReader::new(File::open(path)?);
        chunk::load_id3_chunk::<chunk::WavFormat, _>(&mut file)
    }

    /// Reads an WAV file and returns any present ID3 tag.
    pub fn read_from_wav_file(file: &mut fs::File) -> crate::id3::Result<Tag> {
        chunk::load_id3_chunk::<chunk::WavFormat, _>(file)
    }

    /// Attempts to write the ID3 tag to the writer using the specified version.
    ///
    /// Note that the plain tag is written, regardless of the original contents. To safely encode a
    /// tag to an MP3 file, use `Tag::write_to_file`.
    pub fn write_to(&self, writer: impl io::Write, version: Version) -> crate::id3::Result<()> {
        stream::tag::Encoder::new()
            .version(version)
            .encode(self, writer)
    }

    /// Attempts to write the ID3 tag from the file at the indicated path. If the specified path is
    /// the same path which the tag was read from, then the tag will be written to the padding if
    /// possible.
    pub fn write_to_file(
        &self,
        mut file: impl StorageFile,
        version: Version,
    ) -> crate::id3::Result<()> {
        #[allow(clippy::reversed_empty_ranges)]
        let location = stream::tag::locate_id3v2(&mut file)?.unwrap_or(0..0); // Create a new tag if none could be located.

        let mut storage = PlainStorage::new(file, location);
        let mut w = storage.writer()?;
        stream::tag::Encoder::new()
            .version(version)
            .encode(self, &mut w)?;
        w.flush()?;
        Ok(())
    }

    /// Conventience function for [`write_to_file`].
    pub fn write_to_path(
        &self,
        path: impl AsRef<Path>,
        version: Version,
    ) -> crate::id3::Result<()> {
        let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
        self.write_to_file(file, version)
    }

    /// Overwrite WAV file ID3 chunk in a file
    pub fn write_to_aiff_path(
        &self,
        path: impl AsRef<Path>,
        version: Version,
    ) -> crate::id3::Result<()> {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .truncate(false)
            .open(path)?;
        chunk::write_id3_chunk_file::<chunk::AiffFormat>(&mut file, self, version)?;
        file.flush()?;
        Ok(())
    }

    /// Overwrite AIFF file ID3 chunk in a file. The file must be opened read/write.
    pub fn write_to_aiff_file(
        &self,
        file: &mut fs::File,
        version: Version,
    ) -> crate::id3::Result<()> {
        chunk::write_id3_chunk_file::<chunk::AiffFormat>(file, self, version)
    }

    /// Overwrite WAV file ID3 chunk
    pub fn write_to_wav_path(
        &self,
        path: impl AsRef<Path>,
        version: Version,
    ) -> crate::id3::Result<()> {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .truncate(false)
            .open(path)?;
        chunk::write_id3_chunk_file::<chunk::WavFormat>(&mut file, self, version)?;
        file.flush()?;
        Ok(())
    }

    /// Overwrite AIFF file ID3 chunk in a file. The file must be opened read/write.
    pub fn write_to_wav_file(
        &self,
        file: &mut fs::File,
        version: Version,
    ) -> crate::id3::Result<()> {
        chunk::write_id3_chunk_file::<chunk::WavFormat>(file, self, version)
    }

    /// Returns version of the read tag.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Returns header tag size of the read tag.
    pub fn header_tag_size(&self) -> u64 {
        self.header_tag_size
    }

    /// Returns an iterator over the all frames in the tag.
    ///
    /// # Example
    /// ```
    /// use id3::{Content, Frame, Tag, TagLike};
    ///
    /// let mut tag = Tag::new();
    ///
    /// tag.add_frame(Frame::with_content("TPE1", Content::Text("".to_string())));
    /// tag.add_frame(Frame::with_content("APIC", Content::Text("".to_string())));
    ///
    /// assert_eq!(tag.frames().count(), 2);
    /// ```
    pub fn frames(&'a self) -> impl Iterator<Item = &'a Frame> + 'a {
        self.frames.iter()
    }

    /// Returns an iterator over the extended texts in the tag.
    pub fn extended_texts(&'a self) -> impl Iterator<Item = &'a ExtendedText> + 'a {
        self.frames()
            .filter_map(|frame| frame.content().extended_text())
    }

    /// Returns an iterator over the extended links in the tag.
    pub fn extended_links(&'a self) -> impl Iterator<Item = &'a ExtendedLink> + 'a {
        self.frames()
            .filter_map(|frame| frame.content().extended_link())
    }

    /// Returns an iterator over the [General Encapsulated Object (GEOB)](https://id3.org/id3v2.3.0#General_encapsulated_object) frames in the tag.
    pub fn encapsulated_objects(&'a self) -> impl Iterator<Item = &'a EncapsulatedObject> + 'a {
        self.frames()
            .filter_map(|frame| frame.content().encapsulated_object())
    }
    /// Returns an iterator over the comments in the tag.
    ///
    /// # Example
    /// ```
    /// use id3::{Frame, Tag, TagLike};
    /// use id3::frame::{Content, Comment};
    ///
    /// let mut tag = Tag::new();
    ///
    /// let frame = Frame::with_content("COMM", Content::Comment(Comment {
    ///     lang: "eng".to_owned(),
    ///     description: "key1".to_owned(),
    ///     text: "value1".to_owned()
    /// }));
    /// tag.add_frame(frame);
    ///
    /// let frame = Frame::with_content("COMM", Content::Comment(Comment {
    ///     lang: "eng".to_owned(),
    ///     description: "key2".to_owned(),
    ///     text: "value2".to_owned()
    /// }));
    /// tag.add_frame(frame);
    ///
    /// assert_eq!(tag.comments().count(), 2);
    /// ```
    pub fn comments(&'a self) -> impl Iterator<Item = &'a Comment> + 'a {
        self.frames().filter_map(|frame| frame.content().comment())
    }

    /// Returns an iterator over the lyrics frames in the tag.
    pub fn lyrics(&'a self) -> impl Iterator<Item = &'a Lyrics> + 'a {
        self.frames().filter_map(|frame| frame.content().lyrics())
    }

    /// Returns an iterator over the synchronised lyrics frames in the tag.
    pub fn synchronised_lyrics(&'a self) -> impl Iterator<Item = &'a SynchronisedLyrics> + 'a {
        self.frames()
            .filter_map(|frame| frame.content().synchronised_lyrics())
    }

    /// Returns an iterator over the pictures in the tag.
    ///
    /// # Example
    /// ```
    /// use id3::{Frame, Tag, TagLike};
    /// use id3::frame::{Content, Picture, PictureType};
    ///
    /// let mut tag = Tag::new();
    ///
    /// let picture = Picture {
    ///     mime_type: String::new(),
    ///     picture_type: PictureType::Other,
    ///     description: String::new(),
    ///     data: Vec::new(),
    /// };
    /// tag.add_frame(Frame::with_content("APIC", Content::Picture(picture.clone())));
    /// tag.add_frame(Frame::with_content("APIC", Content::Picture(picture.clone())));
    ///
    /// assert_eq!(tag.pictures().count(), 1);
    /// ```
    pub fn pictures(&'a self) -> impl Iterator<Item = &'a Picture> + 'a {
        self.frames().filter_map(|frame| frame.content().picture())
    }

    /// Returns an iterator over all chapters (CHAP) in the tag.
    ///
    /// # Example
    /// ```
    /// use id3::{Tag, TagLike};
    /// use id3::frame::{Chapter, Content, Frame};
    ///
    /// let mut tag = Tag::new();
    /// tag.add_frame(Chapter{
    ///     element_id: "01".to_string(),
    ///     start_time: 1000,
    ///     end_time: 2000,
    ///     start_offset: 0xff,
    ///     end_offset: 0xff,
    ///     frames: Vec::new(),
    /// });
    /// tag.add_frame(Chapter{
    ///     element_id: "02".to_string(),
    ///     start_time: 2000,
    ///     end_time: 3000,
    ///     start_offset: 0xff,
    ///     end_offset: 0xff,
    ///     frames: Vec::new(),
    /// });
    /// assert_eq!(2, tag.chapters().count());
    /// ```
    pub fn chapters(&self) -> impl Iterator<Item = &Chapter> {
        self.frames().filter_map(|frame| frame.content().chapter())
    }

    /// Returns an iterator over all tables of contents (CTOC) in the tag.
    ///
    /// # Example
    /// ```
    /// use id3::{Tag, TagLike};
    /// use id3::frame::{Chapter, TableOfContents, Content, Frame};
    ///
    /// let mut tag = Tag::new();
    /// tag.add_frame(Chapter{
    ///     element_id: "chap01".to_string(),
    ///     start_time: 1000,
    ///     end_time: 2000,
    ///     start_offset: 0xff,
    ///     end_offset: 0xff,
    ///     frames: Vec::new(),
    /// });
    /// tag.add_frame(TableOfContents{
    ///     element_id: "internalTable01".to_string(),
    ///     top_level: false,
    ///     ordered: false,
    ///     elements: Vec::new(),
    ///     frames: Vec::new(),
    /// });
    /// tag.add_frame(TableOfContents{
    ///     element_id: "01".to_string(),
    ///     top_level: true,
    ///     ordered: true,
    ///     elements: vec!["internalTable01".to_string(),"chap01".to_string()],
    ///     frames: Vec::new(),
    /// });
    /// assert_eq!(2, tag.tables_of_contents().count());
    /// ```
    pub fn tables_of_contents(&self) -> impl Iterator<Item = &TableOfContents> {
        self.frames()
            .filter_map(|frame| frame.content().table_of_contents())
    }
}

impl PartialEq for Tag {
    fn eq(&self, other: &Tag) -> bool {
        self.frames.len() == other.frames.len()
            && self.frames().all(|frame| other.frames.contains(frame))
    }
}

impl FromIterator<Frame> for Tag {
    fn from_iter<I: IntoIterator<Item = Frame>>(iter: I) -> Self {
        Self {
            frames: Vec::from_iter(iter),
            ..Self::default()
        }
    }
}

impl Extend<Frame> for Tag {
    fn extend<I: IntoIterator<Item = Frame>>(&mut self, iter: I) {
        self.frames.extend(iter)
    }
}

impl TagLike for Tag {
    fn frames_vec(&self) -> &Vec<Frame> {
        &self.frames
    }

    fn frames_vec_mut(&mut self) -> &mut Vec<Frame> {
        &mut self.frames
    }
}

impl From<v1::Tag> for Tag {
    fn from(tag_v1: v1::Tag) -> Tag {
        let mut tag = Tag::new();
        if let Some(genre) = tag_v1.genre() {
            tag.set_genre(genre.to_string());
        }
        if !tag_v1.title.is_empty() {
            tag.set_title(tag_v1.title);
        }
        if !tag_v1.artist.is_empty() {
            tag.set_artist(tag_v1.artist);
        }
        if !tag_v1.album.is_empty() {
            tag.set_album(tag_v1.album);
        }
        if !tag_v1.year.is_empty() {
            tag.set_text("TYER", tag_v1.year);
        }
        if !tag_v1.comment.is_empty() {
            tag.add_frame(Comment {
                lang: "eng".to_string(),
                description: "".to_string(),
                text: tag_v1.comment,
            });
        }
        if let Some(track) = tag_v1.track {
            tag.set_track(u32::from(track));
        }
        tag
    }
}
