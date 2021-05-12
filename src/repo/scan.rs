use crate::error;
use anyhow::{anyhow, Result};
use ar::Archive as ArArchive;
use console::style;
use faster_hex::hex_string;
use flate2::read::GzDecoder;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::io::SeekFrom;
use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::Path,
};
use tar::Archive as TarArchive;
use walkdir::{DirEntry, WalkDir};
use xz2::read::XzDecoder;

enum TarFormat {
    Xzip,
    Gzip,
}

fn collect_control<R: Read>(reader: R) -> Result<Vec<u8>> {
    let mut tar = TarArchive::new(reader);
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path_bytes().as_ref() == &b"./control"[..] {
            let mut buf = Vec::new();
            buf.reserve(1024);
            entry.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }

    Err(anyhow!("Could not read control file"))
}

fn open_compressed_control<R: Read>(reader: R, format: &TarFormat) -> Result<Vec<u8>> {
    match format {
        TarFormat::Xzip => collect_control(XzDecoder::new(reader)),
        TarFormat::Gzip => collect_control(GzDecoder::new(reader)),
    }
}

fn determine_format(format: &[u8]) -> Result<TarFormat> {
    if format.ends_with(b".xz") {
        Ok(TarFormat::Xzip)
    } else if format.ends_with(b".gz") {
        Ok(TarFormat::Gzip)
    } else {
        Err(anyhow!("Unknown format: {:?}", format))
    }
}

fn open_deb_simple<R: Read>(reader: R) -> Result<Vec<u8>> {
    let mut deb = ArArchive::new(reader);
    while let Some(entry) = deb.next_entry() {
        if entry.is_err() {
            continue;
        }
        let entry = entry?;
        let filename = entry.header().identifier();
        if filename.starts_with(b"control.tar") {
            let format = determine_format(filename)?;
            let control = open_compressed_control(entry, &format)?;
            return Ok(control);
        }
    }

    Err(anyhow!("data archive not found or format unsupported"))
}

fn scan_single_deb_simple<P: AsRef<Path>>(path: P, root: P) -> Result<Vec<u8>> {
    let mut f = File::open(path.as_ref())?;
    let sha256 = sha256sum(&mut f)?;
    let actual_size = f.seek(SeekFrom::Current(0))?;
    f.seek(SeekFrom::Start(0))?;
    let mut control = open_deb_simple(f)?;
    control.reserve(128);
    if control.ends_with(&b"\n\n"[..]) {
        control.pop();
    }
    let rel_path = path.as_ref().strip_prefix(root)?;
    control.extend(format!("Size: {}\n", actual_size).as_bytes());
    control.extend(format!("Filename: {}\n", rel_path.to_string_lossy()).as_bytes());
    control.extend(b"SHA256: ");
    control.extend(sha256.as_bytes());
    control.extend(b"\n\n");

    Ok(control)
}

/// Calculate the Sha256 checksum of the given stream
pub fn sha256sum<R: Read>(mut reader: R) -> Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher)?;

    Ok(hex_string(&hasher.finalize()))
}

#[inline]
fn is_tarball(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.ends_with(".deb"))
        .unwrap_or(false)
}

pub fn scan_packages_simple(entries: &[DirEntry], root: &Path) -> Vec<u8> {
    entries
        .par_iter()
        .map(|entry| -> Vec<u8> {
            let path = entry.path();
            print!(".");
            std::io::stderr().flush().ok();
            match scan_single_deb_simple(path, root) {
                Ok(entry) => entry,
                Err(err) => {
                    error!("{:?}", err);
                    Vec::new()
                }
            }
        })
        .flatten()
        .collect()
}

pub fn collect_all_packages<P: AsRef<Path>>(path: P) -> Result<Vec<DirEntry>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(path.as_ref()) {
        let entry = entry?;
        if is_tarball(&entry) {
            files.push(entry);
        }
    }

    Ok(files)
}
