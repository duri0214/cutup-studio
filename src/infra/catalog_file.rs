use crate::domain::valueobject::{Catalog, DomainError, Manifest};
use std::{fs, path::Path};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogFileError {
    #[error("{path} を読み込めません: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("{path} は有効な catalog JSON ではありません: {source}")]
    Decode {
        path: String,
        source: serde_json::Error,
    },
    #[error("素材ルートは存在するディレクトリを指定してください: {0}")]
    InvalidMaterialRoot(String),
    #[error(transparent)]
    InvalidCatalog(#[from] DomainError),
    #[error("{path} に manifest を保存できません: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("manifest を JSON に変換できません: {0}")]
    Encode(serde_json::Error),
}

pub fn load_catalog(path: &Path) -> Result<Catalog, CatalogFileError> {
    let display = path.display().to_string();
    let source = fs::read_to_string(path).map_err(|source| CatalogFileError::Read {
        path: display.clone(),
        source,
    })?;
    let catalog =
        serde_json::from_str::<Catalog>(&source).map_err(|source| CatalogFileError::Decode {
            path: display,
            source,
        })?;
    if !Path::new(&catalog.material_root).is_dir() {
        return Err(CatalogFileError::InvalidMaterialRoot(catalog.material_root));
    }
    catalog.validate_metadata()?;
    Ok(catalog)
}

pub fn save_manifest(path: &Path, manifest: &Manifest) -> Result<(), CatalogFileError> {
    let json = serde_json::to_string_pretty(manifest).map_err(CatalogFileError::Encode)?;
    fs::write(path, json).map_err(|source| CatalogFileError::Write {
        path: path.display().to_string(),
        source,
    })
}
