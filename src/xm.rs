use crate::id3::{Tag, TagLike};
use crate::Result;

use wasmer::{imports, Instance, Module, Store, Value};
use wasmer_compiler_cranelift::Cranelift;

const XM_KEY: &[u8] = "ximalayaximalayaximalayaximalaya".as_bytes();
const XM_WASM: &[u8] = include_bytes!("xm.wasm");

pub fn extract_xm_info(reader: impl std::io::Read) -> Result<XMInfo> {
    Tag::read_from(reader)
        .map(|t| t.into())
        .map_err(|e| e.into())
}

pub fn decrypt(xm_info: &XMInfo, content: &[u8]) -> Result<Vec<u8>> {
    let encrypted_data = &content[xm_info.header_size..xm_info.header_size + xm_info.size];
    let iv = xm_info.iv()?;
    let decrypted_data = aes_util::decrypt(encrypted_data, XM_KEY, &iv)?;
    let decrypted_str = String::from_utf8(decrypted_data)?;

    let track_id = format!("{}", xm_info.tracknumber);

    let compiler = Cranelift::new();
    let mut store = Store::new(compiler);
    let module = Module::from_binary(&store, XM_WASM)?;
    let import_object = imports! {};
    let instance = Instance::new(&mut store, &module, &import_object)?;

    let func_a = instance.exports.get_function("a")?;
    let stack_pointer = func_a.call(&mut store, &[Value::I32(-16)])?[0].clone();

    let func_c = instance.exports.get_function("c")?;
    let de_data_offset = func_c.call(&mut store, &[Value::I32(decrypted_str.len() as i32)])?[0]
        .i32()
        .expect("de_data_offset none");

    let track_id_offset = func_c.call(&mut store, &[Value::I32(track_id.len() as i32)])?[0]
        .i32()
        .expect("track_id_offset none");

    let memory_i = instance.exports.get_memory("i")?;
    {
        let view = memory_i.view(&store);
        for (i, b) in decrypted_str.bytes().enumerate() {
            view.write_u8(de_data_offset as u64 + i as u64, b)?;
        }
        for (i, b) in track_id.bytes().enumerate() {
            view.write_u8(track_id_offset as u64 + i as u64, b)?;
        }
    }

    let func_g = instance.exports.get_function("g")?;
    func_g.call(
        &mut store,
        &[
            stack_pointer.clone(),
            Value::I32(de_data_offset),
            Value::I32(decrypted_str.len() as i32),
            Value::I32(track_id_offset),
            Value::I32(track_id.len() as i32),
        ],
    )?;

    let view = memory_i.view(&store);
    let mut buf = [0; 4];
    view.read(
        stack_pointer.i32().expect("stack_pointer none") as u64,
        &mut buf,
    )?;
    let result_pointer = i32::from_le_bytes(buf);
    view.read(
        stack_pointer.i32().expect("stack_pointer none") as u64 + 4,
        &mut buf,
    )?;
    let result_length = i32::from_le_bytes(buf);

    let mem = view.copy_to_vec()?;
    let result_data =
        &mem[result_pointer as usize..result_pointer as usize + result_length as usize];
    let result_data = String::from_utf8(result_data.to_vec())?;
    let full_base64 = format!(
        "{}{}",
        xm_info.encoding_technology.clone().unwrap_or_default(),
        result_data
    );

    let mut decoded_data = base64_util::decode(full_base64)?;
    decoded_data.extend_from_slice(&content[xm_info.header_size + xm_info.size..]);
    Ok(decoded_data)
}

#[derive(Debug, Default, Clone)]
pub struct XMInfo {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    tracknumber: u64,
    size: usize,
    header_size: usize,
    isrc: Option<String>,
    encodedby: Option<String>,
    encoding_technology: Option<String>,
}

impl From<Tag> for XMInfo {
    fn from(value: Tag) -> Self {
        Self {
            title: value
                .get("TIT2")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            artist: value
                .get("TPE1")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            album: value
                .get("TALB")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            tracknumber: value
                .get("TRCK")
                .map(|f| f.content().text().unwrap_or("0").parse().unwrap_or(0))
                .unwrap_or(0),
            size: value
                .get("TSIZ")
                .map(|f| f.content().text().unwrap_or("0").parse().unwrap_or(0))
                .unwrap_or(0),
            header_size: value.header_tag_size() as usize,
            isrc: value
                .get("TSRC")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            encodedby: value
                .get("TENC")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
            encoding_technology: value
                .get("TSSE")
                .map(|f| f.content().text().unwrap_or_default().to_string()),
        }
    }
}

impl XMInfo {
    fn iv(&self) -> Result<Vec<u8>> {
        if let Some(isrc) = &self.isrc {
            hex::decode(isrc).map_err(|e| e.into())
        } else if let Some(encodedby) = &self.encodedby {
            hex::decode(encodedby).map_err(|e| e.into())
        } else {
            Err("no iv".into())
        }
    }

    pub fn file_name(&self, header: &[u8]) -> String {
        let header_chars: Vec<u8> = header
            .iter()
            .filter(|b| (&&0x20u8..=&&0x7Eu8).contains(&b))
            .copied()
            .collect();
        let header_str = String::from_utf8(header_chars)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let ext_name = if header_str.contains("m4a") {
            "m4a"
        } else if header_str.contains("mp3") {
            "mp3"
        } else if header_str.contains("flac") {
            "flac"
        } else if header_str.contains("wav") {
            "wav"
        } else {
            "m4a"
        };

        format!(
            "{} - {} - {}.{}",
            self.artist.clone().unwrap_or_default(),
            self.album.clone().unwrap_or_default(),
            self.title.clone().unwrap_or_default(),
            ext_name
        )
        .replace(['\\', ':', '/', '*', '?', '\"', '<', '>', '|'], "")
    }
}

mod aes_util {
    use crate::Result;
    use aes::cipher::block_padding::Pkcs7;
    use aes::cipher::{BlockDecryptMut, KeyIvInit};

    type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

    pub(super) fn decrypt(ciphertext: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256CbcDec::new(key.into(), iv.into());
        let mut ct_v = ciphertext.to_vec();
        let ct_clone_mut = ct_v.as_mut_slice();
        cipher
            .decrypt_padded_mut::<Pkcs7>(ct_clone_mut)
            .map(|r| r.to_vec())
            .map_err(|_| "unpadded".into())
    }
}

mod base64_util {
    use crate::Result;
    use base64::Engine;

    pub(super) fn decode(input: impl AsRef<[u8]>) -> Result<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(input)
            .map_err(|e| e.into())
    }
}
