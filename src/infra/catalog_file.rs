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
    #[error(
        "{path} は出力済みの manifest です。素材を探すには catalog JSON または SQLite カタログを選んでください"
    )]
    ManifestSelected { path: String },
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
    let catalog = decode_catalog(display, &source)?;
    if !Path::new(&catalog.material_root).is_dir() {
        return Err(CatalogFileError::InvalidMaterialRoot(catalog.material_root));
    }
    catalog.validate_metadata()?;
    Ok(catalog)
}

fn decode_catalog(path: String, source: &str) -> Result<Catalog, CatalogFileError> {
    match serde_json::from_str::<Catalog>(source) {
        Ok(catalog) => Ok(catalog),
        Err(_) if serde_json::from_str::<Manifest>(source).is_ok() => {
            Err(CatalogFileError::ManifestSelected { path })
        }
        Err(source) => Err(CatalogFileError::Decode { path, source }),
    }
}

pub fn save_manifest(path: &Path, manifest: &Manifest) -> Result<(), CatalogFileError> {
    let json = serde_json::to_string_pretty(manifest).map_err(CatalogFileError::Encode)?;
    fs::write(path, json).map_err(|source| CatalogFileError::Write {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_reported_as_the_wrong_input_type() {
        let manifest = Manifest {
            schema_version: 1,
            guide_text: "導入: テスト".into(),
            material_root: "demo".into(),
            entries: Vec::new(),
        };
        let source = serde_json::to_string(&manifest).unwrap();

        assert!(matches!(
            decode_catalog("composition-manifest.json".into(), &source),
            Err(CatalogFileError::ManifestSelected { .. })
        ));
    }
}
