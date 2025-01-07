use core::str;
use faster_hex::hex_string;
use flate2::read::GzDecoder;
use log::error;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
use xz2::read::XzDecoder;

#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum ScanError {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    WalkDirError(#[from] walkdir::Error),
    #[error(transparent)]
    StripPrefixError(#[from] std::path::StripPrefixError),

    #[error("Unknown control.tar compression type: {0}")]
    UnknownControlTarType(String),
    #[error("control.tar not found")]
    MissingControlTar,
    #[error("control file not found")]
    MissingControlFile,
}

pub type Result<T> = std::result::Result<T, ScanError>;

pub(crate) fn collect_all_packages<P: AsRef<Path>>(path: P) -> crate::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(path.as_ref()) {
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .map(|s| s.ends_with(".deb"))
            .unwrap_or(false)
        {
            files.push(entry.into_path());
        }
    }
    Ok(files)
}

pub(crate) fn scan_packages_simple(
    entries: &[PathBuf],
    root: &Path,
) -> crate::Result<Vec<Vec<u8>>> {
    entries
        .par_iter()
        .map(|path| -> crate::Result<Vec<u8>> {
            scan_single_deb_simple(path.as_path(), root)
                .map_err(|err| crate::Error::DebScanError(path.to_owned(), err))
        })
        .collect()
}

fn scan_single_deb_simple<P: AsRef<Path>>(path: P, root: P) -> Result<Vec<u8>> {
    let mut f = File::open(path.as_ref())?;

    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher)?;
    let sha256sum = hex_string(&hasher.finalize());

    let actual_size = f.stream_position()?;
    f.seek(SeekFrom::Start(0))?;

    let mut control = open_deb(f)?;
    control.reserve(128);
    if control.ends_with(&b"\n\n"[..]) {
        control.pop();
    }
    let rel_path = path.as_ref().strip_prefix(root)?;
    control.extend(format!("Size: {}\n", actual_size).as_bytes());
    control.extend(format!("Filename: {}\n", rel_path.to_string_lossy()).as_bytes());
    control.extend(b"SHA256: ");
    control.extend(sha256sum.as_bytes());
    control.extend(b"\n\n");

    Ok(control)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TarCompressionType {
    Xzip,
    Gzip,
    Zstd,
}

fn collect_control<R: Read>(reader: R) -> Result<Vec<u8>> {
    let mut tar = tar::Archive::new(reader);
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path_bytes().as_ref() == &b"./control"[..] {
            let mut buf = Vec::with_capacity(1024);
            entry.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    Err(ScanError::MissingControlFile)
}

fn open_deb<R: Read>(reader: R) -> Result<Vec<u8>> {
    let mut deb = ar::Archive::new(reader);
    while let Some(entry) = deb.next_entry() {
        if entry.is_err() {
            continue;
        }
        let entry = entry?;
        let filename = entry.header().identifier();
        if filename.starts_with(b"control.tar") {
            let format = determine_compression(filename)?;
            let control = open_compressed_control(entry, format)?;
            return Ok(control);
        }
    }
    Err(ScanError::MissingControlTar)
}

fn open_compressed_control<R: Read>(reader: R, format: TarCompressionType) -> Result<Vec<u8>> {
    match format {
        TarCompressionType::Xzip => collect_control(XzDecoder::new(reader)),
        TarCompressionType::Gzip => collect_control(GzDecoder::new(reader)),
        TarCompressionType::Zstd => collect_control(zstd::stream::read::Decoder::new(reader)?),
    }
}

fn determine_compression(format: &[u8]) -> Result<TarCompressionType> {
    if format.ends_with(b".xz") {
        Ok(TarCompressionType::Xzip)
    } else if format.ends_with(b".gz") {
        Ok(TarCompressionType::Gzip)
    } else if format.ends_with(b".zst") {
        Ok(TarCompressionType::Zstd)
    } else {
        Err(ScanError::UnknownControlTarType(
            str::from_utf8(format).unwrap().to_string(),
        ))
    }
}

#[cfg(test)]
mod test {
    use test_log::test;

    use crate::{
        repo::scan::{collect_all_packages, scan_packages_simple, scan_single_deb_simple},
        test::TestDir,
    };

    #[test]
    fn test_collect_all_packages() {
        let testdir = TestDir::from("testdata/simple-repo");
        assert_eq!(
            collect_all_packages(testdir.path().join("debs")).unwrap(),
            vec![
                testdir
                    .path()
                    .join("debs/a/aosc-os-feature-data_20241017.1-0_noarch.deb")
            ]
        );
    }

    #[test]
    fn test_scan_single_deb_simple() {
        let testdir = TestDir::from("testdata/simple-repo");
        assert_eq!(
            String::from_utf8(
                scan_single_deb_simple(
                    testdir
                        .path()
                        .join("debs/a/aosc-os-feature-data_20241017.1-0_noarch.deb"),
                    testdir.path().join("debs")
                )
                .unwrap()
            )
            .unwrap(),
            r##"Package: aosc-os-feature-data
Version: 20241017.1
Architecture: all
Section: misc
Maintainer: AOSC OS Maintainers <maintainers@aosc.io>
Installed-Size: 56
Description: Data defining key AOSC OS features
Description-md5: 248f104b2025bbfc686d24bee09cb14c
Essential: no
X-AOSC-ACBS-Version: 20241023
X-AOSC-Commit: 9c93f94783
X-AOSC-Packager: AOSC OS Maintainers <maintainers@aosc.io>
X-AOSC-Autobuild4-Version: 4.3.27
Size: 1838
Filename: a/aosc-os-feature-data_20241017.1-0_noarch.deb
SHA256: dd386883fa246cc50826cced5df4353b64a490d3f0f487e2d8764b4d7d00151e

"##
        );
    }

    #[test]
    fn test_scan_packages_simple() {
        let testdir = TestDir::from("testdata/simple-repo");
        assert_eq!(
            String::from_utf8(
                scan_packages_simple(
                    &[testdir
                        .path()
                        .join("debs/a/aosc-os-feature-data_20241017.1-0_noarch.deb")],
                    &testdir.path().join("debs")
                )
                .unwrap()
                .concat()
            )
            .unwrap(),
            r##"Package: aosc-os-feature-data
Version: 20241017.1
Architecture: all
Section: misc
Maintainer: AOSC OS Maintainers <maintainers@aosc.io>
Installed-Size: 56
Description: Data defining key AOSC OS features
Description-md5: 248f104b2025bbfc686d24bee09cb14c
Essential: no
X-AOSC-ACBS-Version: 20241023
X-AOSC-Commit: 9c93f94783
X-AOSC-Packager: AOSC OS Maintainers <maintainers@aosc.io>
X-AOSC-Autobuild4-Version: 4.3.27
Size: 1838
Filename: a/aosc-os-feature-data_20241017.1-0_noarch.deb
SHA256: dd386883fa246cc50826cced5df4353b64a490d3f0f487e2d8764b4d7d00151e

"##
        );
    }
}
