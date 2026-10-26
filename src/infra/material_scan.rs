use super::sqlite_catalog::CatalogError;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

pub(super) struct ScannedMaterial {
    pub path: String,
    pub fingerprint: String,
    pub size: i64,
}

pub(super) fn scan(root: &Path) -> Result<Vec<ScannedMaterial>, CatalogError> {
    match root.canonicalize() {
        Ok(actual) if actual != root => return Err(CatalogError::OutsideRoot),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    }
    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn visit(
    root: &Path,
    directory: &Path,
    files: &mut Vec<ScannedMaterial>,
) -> Result<(), CatalogError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        // Junctions/symlinks are not followed: the selected root is the permission boundary.
        if kind.is_symlink() || is_directory_redirect(&entry.metadata()?) {
            continue;
        }
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(root) {
            return Err(CatalogError::OutsideRoot);
        }
        if kind.is_dir() {
            visit(root, &path, files)?;
        } else if kind.is_file() && is_media(&path) {
            let mut file = fs::File::open(&path)?;
            let before = file.metadata()?;
            let mut hash = Sha256::new();
            let mut buffer = [0_u8; 65536];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            let after = file.metadata()?;
            if before.len() != after.len() || before.modified()? != after.modified()? {
                return Err(CatalogError::FileChanged);
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| CatalogError::OutsideRoot)?;
            files.push(ScannedMaterial {
                path: relative
                    .to_str()
                    .ok_or(CatalogError::InvalidPath)?
                    .replace('\\', "/"),
                fingerprint: format!("{:x}", hash.finalize()),
                size: i64::try_from(after.len()).map_err(|_| CatalogError::InvalidPath)?,
            });
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_directory_redirect(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.is_dir() && metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_directory_redirect(_metadata: &fs::Metadata) -> bool {
    false
}

fn is_media(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "mp4"
                    | "mkv"
                    | "mov"
                    | "avi"
                    | "webm"
                    | "m4v"
                    | "wav"
                    | "mp3"
                    | "flac"
                    | "ogg"
                    | "m4a"
                    | "aac"
            )
        })
}
