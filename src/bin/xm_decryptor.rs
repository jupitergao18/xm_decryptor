use std::path::PathBuf;

use xm_decryptor::{xm, Result};

fn main() -> Result<()> {
    let path = PathBuf::from(std::env::args().nth(1).expect("no input path"));
    let mut files = Vec::<PathBuf>::new();
    if path.is_file() {
        files.push(path);
    } else if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                files.push(path);
            }
        }
    }
    let files: Vec<_> = files
        .into_iter()
        .filter(|f| f.extension().unwrap_or_default() == "xm")
        .collect();
    for file in files {
        if let Err(e) = decrypt_file(&file) {
            eprintln!("error: {:?} {:?}", file, e);
        }
    }
    Ok(())
}

fn decrypt_file(file: &PathBuf) -> Result<()> {
    let content = std::fs::read(file)?;

    let xm_info = xm::extract_xm_info(&content[..])?;
    println!("xm_info: {:?}", xm_info);

    let audio = xm::decrypt(&xm_info, &content[..])?;
    let file_name = xm_info.file_name(&audio[..0xFF]);

    let target_path = file.parent().expect("no parent dir").join(file_name);
    println!("target_path: {:?}", target_path);
    std::fs::write(target_path, audio)?;
    Ok(())
}
