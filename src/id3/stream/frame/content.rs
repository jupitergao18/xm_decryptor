use crate::id3::frame::{
    Chapter, Comment, Content, EncapsulatedObject, ExtendedLink, ExtendedText, Lyrics,
    MpegLocationLookupTable, MpegLocationLookupTableReference, Picture, PictureType, Popularimeter,
    Private, SynchronisedLyrics, SynchronisedLyricsType, TableOfContents, TimestampFormat, Unknown,
};
use crate::id3::stream::encoding::Encoding;
use crate::id3::stream::frame;
use crate::id3::tag::Version;
use crate::id3::{Error, ErrorKind};
use std::convert::{TryFrom, TryInto};
use std::io;
use std::iter;
use std::mem::size_of;

struct Encoder<W: io::Write> {
    w: W,
    version: Version,
    encoding: Encoding,
}

impl<W: io::Write> Encoder<W> {
    fn bytes(&mut self, bytes: impl AsRef<[u8]>) -> crate::id3::Result<()> {
        let bytes = bytes.as_ref();
        self.w.write_all(bytes)?;
        Ok(())
    }

    fn byte(&mut self, b: u8) -> crate::id3::Result<()> {
        self.bytes([b])
    }

    fn uint16(&mut self, int: u16) -> crate::id3::Result<()> {
        self.bytes(int.to_be_bytes())
    }

    fn uint24(&mut self, int: u32) -> crate::id3::Result<()> {
        self.bytes(&int.to_be_bytes()[1..])
    }

    fn uint32(&mut self, int: u32) -> crate::id3::Result<()> {
        self.bytes(int.to_be_bytes())
    }

    fn delim(&mut self) -> crate::id3::Result<()> {
        self.bytes(match self.encoding {
            Encoding::Latin1 | Encoding::UTF8 => &[0][..],
            Encoding::UTF16 | Encoding::UTF16BE => &[0, 0][..],
        })
    }

    fn string(&mut self, string: &str) -> crate::id3::Result<()> {
        self.string_with_other_encoding(self.encoding, string)
    }

    fn string_with_other_encoding(
        &mut self,
        encoding: Encoding,
        string: &str,
    ) -> crate::id3::Result<()> {
        self.bytes(encoding.encode(string))
    }

    fn encoding(&mut self) -> crate::id3::Result<()> {
        self.byte(match self.encoding {
            Encoding::Latin1 => 0,
            Encoding::UTF16 => 1,
            Encoding::UTF16BE => 2,
            Encoding::UTF8 => 3,
        })
    }

    fn text_content(&mut self, content: &str) -> crate::id3::Result<()> {
        self.encoding()?;
        match self.version {
            Version::Id3v22 | Version::Id3v23 => self.string(&content.replace('\0', "/")),
            Version::Id3v24 => self.string(content),
        }
    }

    fn extended_text_content(&mut self, content: &ExtendedText) -> crate::id3::Result<()> {
        self.encoding()?;
        self.string(&content.description)?;
        self.delim()?;
        self.string(&content.value)
    }

    fn link_content(&mut self, content: &str) -> crate::id3::Result<()> {
        self.bytes(content.as_bytes())
    }

    fn extended_link_content(&mut self, content: &ExtendedLink) -> crate::id3::Result<()> {
        self.encoding()?;
        self.string(&content.description)?;
        self.delim()?;
        self.bytes(content.link.as_bytes())
    }

    fn encapsulated_object_content(
        &mut self,
        content: &EncapsulatedObject,
    ) -> crate::id3::Result<()> {
        self.encoding()?;
        self.bytes(content.mime_type.as_bytes())?;
        self.byte(0)?;
        self.string(&content.filename)?;
        self.delim()?;
        self.string(&content.description)?;
        self.delim()?;
        self.bytes(&content.data)?;
        Ok(())
    }

    fn lyrics_content(&mut self, content: &Lyrics) -> crate::id3::Result<()> {
        self.encoding()?;
        self.bytes(
            content
                .lang
                .bytes()
                .chain(iter::repeat(b' '))
                .take(3)
                .collect::<Vec<u8>>(),
        )?;
        self.string(&content.description)?;
        self.delim()?;
        self.string(&content.text)
    }

    fn synchronised_lyrics_content(
        &mut self,
        content: &SynchronisedLyrics,
    ) -> crate::id3::Result<()> {
        // SYLT frames are really weird because they encode the text encoding and delimiters in a
        // different way.
        let encoding = match self.encoding {
            Encoding::Latin1 => Encoding::Latin1,
            _ => Encoding::UTF16,
        };
        self.byte(match encoding {
            Encoding::Latin1 => 0,
            Encoding::UTF16 => 1,
            _ => unreachable!(),
        })?;
        self.bytes(
            &content
                .lang
                .bytes()
                .chain(iter::repeat(b' '))
                .take(3)
                .collect::<Vec<u8>>(),
        )?;
        self.byte(match content.timestamp_format {
            TimestampFormat::Mpeg => 1,
            TimestampFormat::Ms => 2,
        })?;
        self.byte(match content.content_type {
            SynchronisedLyricsType::Other => 0,
            SynchronisedLyricsType::Lyrics => 1,
            SynchronisedLyricsType::Transcription => 2,
            SynchronisedLyricsType::PartName => 3,
            SynchronisedLyricsType::Event => 4,
            SynchronisedLyricsType::Chord => 5,
            SynchronisedLyricsType::Trivia => 6,
        })?;
        let text_delim: &[u8] = match encoding {
            Encoding::Latin1 => &[0],
            Encoding::UTF16 => &[0, 0],
            _ => unreachable!(),
        };
        // Description
        self.string_with_other_encoding(encoding, &content.description)?;
        self.bytes(text_delim)?;
        for (timestamp, text) in &content.content {
            self.string_with_other_encoding(encoding, text)?;
            self.bytes(text_delim)?;
            self.uint32(*timestamp)?;
        }
        self.byte(0)
    }

    fn comment_content(&mut self, content: &Comment) -> crate::id3::Result<()> {
        self.encoding()?;
        self.bytes(
            content
                .lang
                .bytes()
                .chain(iter::repeat(b' '))
                .take(3)
                .collect::<Vec<u8>>(),
        )?;
        self.string(&content.description)?;
        self.delim()?;
        self.string(&content.text)
    }

    fn popularimeter_content(&mut self, content: &Popularimeter) -> crate::id3::Result<()> {
        self.string_with_other_encoding(Encoding::Latin1, &content.user)?;
        self.byte(0)?;
        self.byte(content.rating)?;
        let counter_bin = content.counter.to_be_bytes();
        let i = counter_bin
            .iter()
            .position(|b| *b != 0)
            .unwrap_or(size_of::<u64>());
        self.bytes(&counter_bin[i..])
    }

    fn picture_content_v2(&mut self, content: &Picture) -> crate::id3::Result<()> {
        self.encoding()?;
        let format = match &content.mime_type[..] {
            "image/jpeg" | "image/jpg" => "JPG",
            "image/png" => "PNG",
            _ => return Err(Error::new(ErrorKind::Parsing, "unsupported MIME type")),
        };
        self.bytes(format.as_bytes())?;
        self.byte(u8::from(content.picture_type))?;
        self.string(&content.description)?;
        self.delim()?;
        self.bytes(&content.data)
    }

    fn picture_content_v3(&mut self, content: &Picture) -> crate::id3::Result<()> {
        self.encoding()?;
        self.bytes(content.mime_type.as_bytes())?;
        self.byte(0)?;
        self.byte(u8::from(content.picture_type))?;
        self.string(&content.description)?;
        self.delim()?;
        self.bytes(&content.data)
    }

    fn picture_content(&mut self, content: &Picture) -> crate::id3::Result<()> {
        match self.version {
            Version::Id3v22 => self.picture_content_v2(content),
            Version::Id3v23 | Version::Id3v24 => self.picture_content_v3(content),
        }
    }

    fn chapter_content(&mut self, content: &Chapter) -> crate::id3::Result<()> {
        self.string_with_other_encoding(Encoding::Latin1, &content.element_id)?;
        self.byte(0)?;
        self.uint32(content.start_time)?;
        self.uint32(content.end_time)?;
        self.uint32(content.start_offset)?;
        self.uint32(content.end_offset)?;
        for frame in &content.frames {
            frame::encode(&mut self.w, frame, self.version, false)?;
        }
        Ok(())
    }

    fn mpeg_location_lookup_table_content(
        &mut self,
        content: &MpegLocationLookupTable,
    ) -> crate::id3::Result<()> {
        let ref_packed_size = content.bits_for_bytes + content.bits_for_millis;
        if ref_packed_size % 4 != 0 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "MLLT bits_for_bytes + bits_for_millis must be a multiple of 4",
            ));
        } else if ref_packed_size > 64 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "MLLT bits_for_bytes + bits_for_millis must be <= 64",
            ));
        }

        self.uint16(content.frames_between_reference)?;
        self.uint24(content.bytes_between_reference)?;
        self.uint24(content.millis_between_reference)?;
        self.byte(content.bits_for_bytes)?;
        self.byte(content.bits_for_millis)?;

        let mut carry = 0u64;
        let mut carry_bits = 0usize;
        for r in &content.references {
            for (ref_field, bits) in [
                (r.deviate_bytes, content.bits_for_bytes),
                (r.deviate_millis, content.bits_for_millis),
            ] {
                let deviate = u64::from(ref_field) & ((1 << bits) - 1);
                carry |= deviate << (64 - usize::from(bits) - carry_bits);
                carry_bits += usize::from(bits);
                let shift_out_bytes = carry_bits / 8;
                self.bytes(&carry.to_be_bytes()[..shift_out_bytes])?;
                carry <<= shift_out_bytes * 8;
                carry_bits -= shift_out_bytes * 8;
            }
        }
        debug_assert!(carry_bits < 8);
        if carry_bits > 0 {
            self.byte((carry >> 56) as u8)?;
        }
        Ok(())
    }

    fn private_content(&mut self, content: &Private) -> crate::id3::Result<()> {
        self.bytes(content.owner_identifier.as_bytes())?;
        self.bytes(content.private_data.as_slice())?;
        Ok(())
    }

    fn table_of_contents_content(&mut self, content: &TableOfContents) -> crate::id3::Result<()> {
        self.string_with_other_encoding(Encoding::Latin1, &content.element_id)?;
        self.byte(0)?;
        let top_level_flag = match content.top_level {
            true => 2,
            false => 0,
        };

        let ordered_flag = match content.ordered {
            true => 1,
            false => 0,
        };
        self.byte(top_level_flag | ordered_flag)?;
        self.byte(content.elements.len() as u8)?;

        for element in &content.elements {
            self.string_with_other_encoding(Encoding::Latin1, element)?;
            self.byte(0)?;
        }
        for frame in &content.frames {
            frame::encode(&mut self.w, frame, self.version, false)?;
        }
        Ok(())
    }
}

pub fn encode(
    mut writer: impl io::Write,
    content: &Content,
    version: Version,
    encoding: Encoding,
) -> crate::id3::Result<usize> {
    let mut buf = Vec::new();

    let mut encoder = Encoder {
        w: &mut buf,
        version,
        encoding,
    };
    match content {
        Content::Text(c) => encoder.text_content(c)?,
        Content::ExtendedText(c) => encoder.extended_text_content(c)?,
        Content::Link(c) => encoder.link_content(c)?,
        Content::ExtendedLink(c) => encoder.extended_link_content(c)?,
        Content::EncapsulatedObject(c) => encoder.encapsulated_object_content(c)?,
        Content::Lyrics(c) => encoder.lyrics_content(c)?,
        Content::SynchronisedLyrics(c) => encoder.synchronised_lyrics_content(c)?,
        Content::Comment(c) => encoder.comment_content(c)?,
        Content::Popularimeter(c) => encoder.popularimeter_content(c)?,
        Content::Picture(c) => encoder.picture_content(c)?,
        Content::Chapter(c) => encoder.chapter_content(c)?,
        Content::MpegLocationLookupTable(c) => encoder.mpeg_location_lookup_table_content(c)?,
        Content::Private(c) => encoder.private_content(c)?,
        Content::TableOfContents(c) => encoder.table_of_contents_content(c)?,
        Content::Unknown(c) => encoder.bytes(&c.data)?,
    };

    writer.write_all(&buf)?;
    Ok(buf.len())
}

pub fn decode(
    id: &str,
    version: Version,
    mut reader: impl io::Read,
) -> crate::id3::Result<(Content, Option<Encoding>)> {
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;
    let decoder = Decoder {
        r: &mut data,
        version,
    };

    let mut encoding = None;
    let content = match id {
        "PIC" => {
            if cfg!(feature = "decode_picture") {
                decoder.picture_content_v2()
            } else {
                Ok(Content::Unknown(Unknown { data, version }))
            }
        }
        "APIC" => {
            if cfg!(feature = "decode_picture") {
                decoder.picture_content_v3()
            } else {
                Ok(Content::Unknown(Unknown { data, version }))
            }
        }
        "TXXX" | "TXX" => {
            let (content, enc) = decoder.extended_text_content()?;
            encoding = Some(enc);
            Ok(content)
        }
        "WXXX" | "WXX" => decoder.extended_link_content(),
        "COMM" | "COM" => decoder.comment_content(),
        "POPM" | "POP" => decoder.popularimeter_content(),
        "USLT" | "ULT" => decoder.lyrics_content(),
        "SYLT" | "SLT" => decoder.synchronised_lyrics_content(),
        "GEOB" | "GEO" => {
            let (content, enc) = decoder.encapsulated_object_content()?;
            encoding = Some(enc);
            Ok(content)
        }
        id if id.starts_with('T') => decoder.text_content(),
        id if id.starts_with('W') => decoder.link_content(),
        "GRP1" => decoder.text_content(),
        "CHAP" => decoder.chapter_content(),
        "MLLT" => decoder.mpeg_location_lookup_table_content(),
        "PRIV" => decoder.private_content(),
        "CTOC" => decoder.table_of_contents_content(),
        _ => Ok(Content::Unknown(Unknown { data, version })),
    }?;
    Ok((content, encoding))
}

struct Decoder<'a> {
    r: &'a [u8],
    version: Version,
}

impl<'a> Decoder<'a> {
    fn bytes(&mut self, len: usize) -> crate::id3::Result<&'a [u8]> {
        if len > self.r.len() {
            return Err(Error::new(
                ErrorKind::Parsing,
                "Insufficient data to decode bytes",
            ));
        }
        let (head, tail) = self.r.split_at(len);
        self.r = tail;
        Ok(head)
    }

    fn byte(&mut self) -> crate::id3::Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn uint16(&mut self) -> crate::id3::Result<u16> {
        let b = self.bytes(2)?;
        let a = b.try_into().unwrap();
        Ok(u16::from_be_bytes(a))
    }

    fn uint24(&mut self) -> crate::id3::Result<u32> {
        let b3 = self.bytes(3)?;
        let mut b4 = [0; 4];
        b4[1..4].copy_from_slice(b3);
        Ok(u32::from_be_bytes(b4))
    }

    fn uint32(&mut self) -> crate::id3::Result<u32> {
        let b = self.bytes(4)?;
        let a = b.try_into().unwrap();
        Ok(u32::from_be_bytes(a))
    }

    fn string_until_eof(&mut self, encoding: Encoding) -> crate::id3::Result<String> {
        encoding.decode(self.r)
    }

    fn string_delimited(&mut self, encoding: Encoding) -> crate::id3::Result<String> {
        let delim = find_delim(encoding, self.r, 0)
            .ok_or_else(|| Error::new(ErrorKind::Parsing, "delimiter not found"))?;
        let delim_len = delim_len(encoding);
        let b = self.bytes(delim)?;
        self.bytes(delim_len)?; // Skip.
        encoding.decode(b)
    }

    fn string_fixed(&mut self, bytes_len: usize) -> crate::id3::Result<String> {
        let s = self.bytes(bytes_len)?;
        Encoding::Latin1.decode(s)
    }

    fn encoding(&mut self) -> crate::id3::Result<Encoding> {
        match self.byte()? {
            0 => Ok(Encoding::Latin1),
            1 => Ok(Encoding::UTF16),
            2 => Ok(Encoding::UTF16BE),
            3 => Ok(Encoding::UTF8),
            _ => Err(Error::new(ErrorKind::Parsing, "unknown encoding")),
        }
    }

    fn text_content(mut self) -> crate::id3::Result<Content> {
        let encoding = self.encoding()?;
        let (end, _) = match self.version {
            Version::Id3v24 => match find_closing_delim(encoding, self.r) {
                Some(i) => (i, i + delim_len(encoding)),
                None => (self.r.len(), self.r.len()),
            },
            _ => match find_delim(encoding, self.r, 0) {
                Some(i) => (i, i + delim_len(encoding)),
                None => (self.r.len(), self.r.len()),
            },
        };
        let text = encoding.decode(self.bytes(end)?)?;
        let text = match self.version {
            Version::Id3v22 | Version::Id3v23 => text.replace('/', "\0"),
            Version::Id3v24 => text,
        };
        Ok(Content::Text(text))
    }

    fn link_content(self) -> crate::id3::Result<Content> {
        Ok(Content::Link(String::from_utf8(self.r.to_vec())?))
    }

    fn picture_type(&mut self) -> crate::id3::Result<PictureType> {
        Ok(match self.byte()? {
            0 => PictureType::Other,
            1 => PictureType::Icon,
            2 => PictureType::OtherIcon,
            3 => PictureType::CoverFront,
            4 => PictureType::CoverBack,
            5 => PictureType::Leaflet,
            6 => PictureType::Media,
            7 => PictureType::LeadArtist,
            8 => PictureType::Artist,
            9 => PictureType::Conductor,
            10 => PictureType::Band,
            11 => PictureType::Composer,
            12 => PictureType::Lyricist,
            13 => PictureType::RecordingLocation,
            14 => PictureType::DuringRecording,
            15 => PictureType::DuringPerformance,
            16 => PictureType::ScreenCapture,
            17 => PictureType::BrightFish,
            18 => PictureType::Illustration,
            19 => PictureType::BandLogo,
            20 => PictureType::PublisherLogo,
            b => PictureType::Undefined(b),
        })
    }

    fn picture_content_v2(mut self) -> crate::id3::Result<Content> {
        let encoding = self.encoding()?;
        let mime_type = match self.string_fixed(3)?.as_str() {
            "PNG" => "image/png".to_string(),
            "JPG" => "image/jpeg".to_string(),
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedFeature,
                    "can't determine MIME type for image format",
                ))
            }
        };
        let picture_type = self.picture_type()?;
        let description = self.string_delimited(encoding)?;
        let data = self.r.to_vec();
        Ok(Content::Picture(Picture {
            mime_type,
            picture_type,
            description,
            data,
        }))
    }

    fn picture_content_v3(mut self) -> crate::id3::Result<Content> {
        let encoding = self.encoding()?;
        let mime_type = self.string_delimited(Encoding::Latin1)?;
        let picture_type = self.picture_type()?;
        let description = self.string_delimited(encoding)?;
        let data = self.r.to_vec();
        Ok(Content::Picture(Picture {
            mime_type,
            picture_type,
            description,
            data,
        }))
    }

    fn comment_content(mut self) -> crate::id3::Result<Content> {
        let encoding = self.encoding()?;
        let lang = self.string_fixed(2)?;
        let description = self.string_delimited(encoding)?;
        let text = self.string_until_eof(encoding)?;
        Ok(Content::Comment(Comment {
            lang,
            description,
            text,
        }))
    }

    fn popularimeter_content(mut self) -> crate::id3::Result<Content> {
        let user = self.string_delimited(Encoding::Latin1)?;
        let rating = self.byte()?;
        let counter = {
            let r = match self.r.len() {
                0..=8 => self.r,
                9.. => &self.r[..8],
                _ => unreachable!(),
            };
            let mut bin = [0; 8];
            bin[8 - r.len()..].copy_from_slice(r);
            u64::from_be_bytes(bin)
        };
        Ok(Content::Popularimeter(Popularimeter {
            user,
            rating,
            counter,
        }))
    }

    fn extended_text_content(mut self) -> crate::id3::Result<(Content, Encoding)> {
        let encoding = self.encoding()?;
        let description = self.string_delimited(encoding)?;
        let value = self.string_until_eof(encoding)?;
        Ok((
            Content::ExtendedText(ExtendedText { description, value }),
            encoding,
        ))
    }

    fn extended_link_content(mut self) -> crate::id3::Result<Content> {
        let encoding = self.encoding()?;
        let description = self.string_delimited(encoding)?;
        let link = self.string_until_eof(Encoding::Latin1)?;
        Ok(Content::ExtendedLink(ExtendedLink { description, link }))
    }

    fn encapsulated_object_content(mut self) -> crate::id3::Result<(Content, Encoding)> {
        let encoding = self.encoding()?;
        let mime_type = self.string_delimited(Encoding::Latin1)?;
        let filename = self.string_delimited(encoding)?;
        let description = self.string_delimited(encoding)?;
        let data = self.r.to_vec();
        Ok((
            Content::EncapsulatedObject(EncapsulatedObject {
                mime_type,
                filename,
                description,
                data,
            }),
            encoding,
        ))
    }

    fn lyrics_content(mut self) -> crate::id3::Result<Content> {
        let encoding = self.encoding()?;
        let lang = self.string_fixed(2)?;
        let description = self.string_delimited(encoding)?;
        let text = self.string_until_eof(encoding)?;
        Ok(Content::Lyrics(Lyrics {
            lang,
            description,
            text,
        }))
    }

    fn synchronised_lyrics_content(mut self) -> crate::id3::Result<Content> {
        let (encoding, text_delim) = match self.byte()? {
            0 => (Encoding::Latin1, &[0][..]),
            1 => (Encoding::UTF16, &[0, 0][..]),
            _ => return Err(Error::new(ErrorKind::Parsing, "invalid SYLT encoding")),
        };

        let lang = self.string_fixed(2)?;
        let timestamp_format = match self.byte()? {
            1 => TimestampFormat::Mpeg,
            2 => TimestampFormat::Ms,
            _ => {
                return Err(Error::new(
                    ErrorKind::Parsing,
                    "invalid SYLT timestamp format",
                ))
            }
        };
        let content_type = match self.byte()? {
            0 => SynchronisedLyricsType::Other,
            1 => SynchronisedLyricsType::Lyrics,
            2 => SynchronisedLyricsType::Transcription,
            3 => SynchronisedLyricsType::PartName,
            4 => SynchronisedLyricsType::Event,
            5 => SynchronisedLyricsType::Chord,
            6 => SynchronisedLyricsType::Trivia,
            _ => return Err(Error::new(ErrorKind::Parsing, "invalid SYLT content type")),
        };

        let mut description = None;
        let mut content = Vec::new();
        while let Some(i) = self
            .r
            .chunks(text_delim.len())
            .position(|w| w == text_delim)
        {
            let i = i * text_delim.len();
            let text = encoding.decode(&self.r[..i])?;

            self.r = &self.r[i + text_delim.len()..];

            // Read description
            if description.is_none() {
                description = Some(text);
                continue;
            }

            let timestamp = self.uint32()?;
            content.push((timestamp, text));
        }

        Ok(Content::SynchronisedLyrics(SynchronisedLyrics {
            lang,
            timestamp_format,
            content_type,
            content,
            description: description.unwrap_or_default(),
        }))
    }

    fn chapter_content(mut self) -> crate::id3::Result<Content> {
        let element_id = self.string_delimited(Encoding::Latin1)?;
        let start_time = self.uint32()?;
        let end_time = self.uint32()?;
        let start_offset = self.uint32()?;
        let end_offset = self.uint32()?;
        let mut frames = Vec::new();
        while let Some((_advance, frame)) = frame::decode(&mut self.r, self.version)? {
            frames.push(frame);
        }
        Ok(Content::Chapter(Chapter {
            element_id,
            start_time,
            end_time,
            start_offset,
            end_offset,
            frames,
        }))
    }

    fn mpeg_location_lookup_table_content(mut self) -> crate::id3::Result<Content> {
        let frames_between_reference = self.uint16()?;
        let bytes_between_reference = self.uint24()?;
        let millis_between_reference = self.uint24()?;
        let bits_for_bytes = self.byte()?;
        let bits_for_millis = self.byte()?;

        if bits_for_bytes == 0 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "MLLT bits_for_bytes must be > 0",
            ));
        } else if bits_for_millis == 0 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "MLLT bits_for_millis must be > 0",
            ));
        }

        let bits_for_bytes_us = usize::from(bits_for_bytes);
        let bits_for_millis_us = usize::from(bits_for_millis);
        let mut references = Vec::new();
        let mut carry = 0u64;
        let mut carry_bits = 0usize;
        let mut bytes = self.r.iter().copied().peekable();
        while bytes.peek().is_some() {
            // Load enough bytes to shift the next reference from.
            for b in bytes
                .by_ref()
                .take((bits_for_bytes_us + bits_for_millis_us) / 8)
            {
                carry |= u64::from(b) << (64 - carry_bits - 8);
                carry_bits += 8;
            }
            // Shift 2 deviation fields from the carry accumulator.
            let mut deviations = [0u32; 2];
            for (i, bits_us) in [bits_for_bytes_us, bits_for_millis_us]
                .into_iter()
                .enumerate()
            {
                if carry_bits < bits_us {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        format!(
                            "MLLT not enough bits left for reference: {}<{}",
                            carry_bits, bits_us
                        ),
                    ));
                }
                deviations[i] = u32::try_from(carry >> (64 - bits_us)).unwrap();
                carry <<= bits_us;
                carry_bits -= bits_us;
            }
            let [deviate_bytes, deviate_millis] = deviations;
            references.push(MpegLocationLookupTableReference {
                deviate_bytes,
                deviate_millis,
            });
        }

        Ok(Content::MpegLocationLookupTable(MpegLocationLookupTable {
            frames_between_reference,
            bytes_between_reference,
            millis_between_reference,
            bits_for_bytes,
            bits_for_millis,
            references,
        }))
    }

    fn private_content(mut self) -> crate::id3::Result<Content> {
        let owner_identifier = self.string_delimited(Encoding::Latin1)?;
        let private_data = self.r.to_vec();

        Ok(Content::Private(Private {
            owner_identifier,
            private_data,
        }))
    }
    fn table_of_contents_content(mut self) -> crate::id3::Result<Content> {
        let element_id = self.string_delimited(Encoding::Latin1)?;
        let flags = self.byte()?;
        let top_level = matches!(!!(flags & 2), 2);
        let ordered = matches!(!!(flags & 1), 1);
        let element_count = self.byte()?;
        let mut elements = Vec::new();
        for _ in 0..element_count {
            elements.push(self.string_delimited(Encoding::Latin1)?);
        }
        let mut frames = Vec::new();
        while let Some((_advance, frame)) = frame::decode(&mut self.r, self.version)? {
            frames.push(frame);
        }
        Ok(Content::TableOfContents(TableOfContents {
            element_id,
            top_level,
            ordered,
            elements,
            frames,
        }))
    }
}

/// Returns the index of the first delimiter for the specified encoding.
fn find_delim(encoding: Encoding, data: &[u8], index: usize) -> Option<usize> {
    let mut i = index;
    match encoding {
        Encoding::Latin1 | Encoding::UTF8 => {
            if i >= data.len() {
                return None;
            }

            for c in data[i..].iter() {
                if *c == 0 {
                    break;
                }
                i += 1;
            }

            if i == data.len() {
                // delimiter was not found
                return None;
            }

            Some(i)
        }
        Encoding::UTF16 | Encoding::UTF16BE => {
            while i + 1 < data.len() && (data[i] != 0 || data[i + 1] != 0) {
                i += 2;
            }

            if i + 1 >= data.len() {
                // delimiter was not found
                return None;
            }

            Some(i)
        }
    }
}

/// Returns the index of the last delimiter for the specified encoding.
pub fn find_closing_delim(encoding: Encoding, data: &[u8]) -> Option<usize> {
    let mut i = data.len();
    match encoding {
        Encoding::Latin1 | Encoding::UTF8 => {
            i = i.checked_sub(1)?;
            while i > 0 {
                if data[i] != 0 {
                    return if (i + 1) == data.len() {
                        None
                    } else {
                        Some(i + 1)
                    };
                }
                i -= 1;
            }
            None
        }
        Encoding::UTF16 | Encoding::UTF16BE => {
            i = i.checked_sub(2)?;
            i -= i % 2; // align to 2-byte boundary
            while i > 1 {
                if data[i] != 0 || data[i + 1] != 0 {
                    return if (i + 2) == data.len() {
                        None
                    } else {
                        Some(i + 2)
                    };
                }
                i -= 2;
            }
            None
        }
    }
}

/// Returns the delimiter length for the specified encoding.
fn delim_len(encoding: Encoding) -> usize {
    match encoding {
        Encoding::Latin1 | Encoding::UTF8 => 1,
        Encoding::UTF16 | Encoding::UTF16BE => 2,
    }
}
