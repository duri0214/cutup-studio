use crate::domain::valueobject::Manifest;
use std::{fs, path::Path};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogFileError {
    #[error("{path} に manifest を保存できません: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("manifest を JSON に変換できません: {0}")]
    Encode(serde_json::Error),
}

pub fn save_manifest(path: &Path, manifest: &Manifest) -> Result<(), CatalogFileError> {
    let json = serde_json::to_string_pretty(manifest).map_err(CatalogFileError::Encode)?;
    fs::write(path, json).map_err(|source| CatalogFileError::Write {
        path: path.display().to_string(),
        source,
    })
}
