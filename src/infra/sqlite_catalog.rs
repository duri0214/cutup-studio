use super::material_scan;
use crate::domain::valueobject::{Catalog, CatalogMaterial, SCHEMA_VERSION, catalog::*};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use thiserror::Error;

const APPLICATION_ID: i64 = 1129665603;
const CATALOG_VERSION: i64 = 2;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("カタログの読み書きに失敗しました: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("素材または保存先にアクセスできません: {0}")]
    Io(#[from] std::io::Error),
    #[error("メタデータを読み書きできません: {0}")]
    Json(#[from] serde_json::Error),
    #[error(
        "このカタログは古い形式です。素材フォルダ内の古い material-catalog.sqlite を別名にしてから、NEW で作り直してください"
    )]
    UnsupportedSchema,
    #[error("素材ルート外への参照は登録できません")]
    OutsideRoot,
    #[error("基準フォルダに materials フォルダがありません: {0}")]
    MissingMaterialsDirectory(String),
    #[error("素材パスは UTF-8 で表せる実在のフォルダを指定してください")]
    InvalidPath,
    #[error("走査中に素材が変更されました。OPENで基準フォルダを開き直してください")]
    FileChanged,
    #[error("素材を選択してください")]
    NoMaterialSelected,
}

pub struct SqliteCatalog {
    connection: Connection,
    root: PathBuf,
}

impl SqliteCatalog {
    pub fn create(path: &Path, root: &Path) -> Result<Self, CatalogError> {
        let root = normalize_path(root.canonicalize()?);
        if !root.is_dir() {
            return Err(CatalogError::InvalidPath);
        }
        materials_directory(&root)?;
        let root_text = root.to_str().ok_or(CatalogError::InvalidPath)?;
        let reservation = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        drop(reservation);
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        configure(&connection)?;
        let tx = connection.transaction()?;
        tx.execute_batch(include_str!("catalog_schema.sql"))?;
        tx.execute(
            "INSERT INTO catalog(id,material_root) VALUES(1,?1)",
            [root_text],
        )?;
        tx.commit()?;
        Ok(Self { connection, root })
    }

    pub fn open(path: &Path) -> Result<Self, CatalogError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        configure(&connection)?;
        let app: i64 = connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if app != APPLICATION_ID || version != CATALOG_VERSION {
            return Err(CatalogError::UnsupportedSchema);
        }
        let root: String =
            connection.query_row("SELECT material_root FROM catalog WHERE id=1", [], |row| {
                row.get(0)
            })?;
        let root = normalize_path(root.into());
        materials_directory(&root)?;
        Ok(Self { connection, root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rescan(&mut self) -> Result<ScanReport, CatalogError> {
        let mut files = material_scan::scan(&materials_directory(&self.root)?)?;
        for file in &mut files {
            file.path = format!("materials/{}", file.path);
        }
        let old: BTreeMap<_, _> = self
            .materials()?
            .into_iter()
            .map(|material| (material.source_path.clone(), material))
            .collect();
        let tx = self.connection.transaction()?;
        let mut report = ScanReport::default();
        let mut remaining = old;
        for file in files {
            if let Some(previous) = remaining.remove(&file.path) {
                if previous.fingerprint != file.fingerprint {
                    report.changed += 1;
                }
                tx.execute(
                    "UPDATE materials SET fingerprint=?2,size_bytes=?3,
                     updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                    params![previous.id, file.fingerprint, file.size],
                )?;
                continue;
            }
            let matching_paths = remaining
                .iter()
                .filter(|(_, material)| material.fingerprint == file.fingerprint)
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            if let [previous_path] = matching_paths.as_slice() {
                let previous = remaining
                    .remove(previous_path)
                    .expect("matching material must remain available for this scan");
                tx.execute(
                    "UPDATE materials SET source_path=?2,fingerprint=?3,size_bytes=?4,
                     updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                    params![previous.id, file.path, file.fingerprint, file.size],
                )?;
                report.moved += 1;
            } else {
                tx.execute(
                    "INSERT INTO materials(source_path,fingerprint,size_bytes) VALUES(?1,?2,?3)",
                    params![file.path, file.fingerprint, file.size],
                )?;
                report.added += 1;
            }
        }
        for material in remaining.values() {
            tx.execute(
                "DELETE FROM material_attributes WHERE material_id=?1",
                [material.id],
            )?;
            tx.execute("DELETE FROM materials WHERE id=?1", [material.id])?;
            report.removed += 1;
        }
        tx.commit()?;
        Ok(report)
    }

    pub fn materials(&self) -> Result<Vec<Material>, CatalogError> {
        let mut statement = self
            .connection
            .prepare("SELECT id,source_path,fingerprint FROM materials ORDER BY source_path")?;
        Ok(statement
            .query_map([], |row| {
                Ok(Material {
                    id: row.get(0)?,
                    source_path: row.get(1)?,
                    fingerprint: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?)
    }

    pub fn save_metadata(
        &mut self,
        material_id: i64,
        metadata: &Metadata,
    ) -> Result<(), CatalogError> {
        let tx = self.connection.transaction()?;
        let fingerprint = material_fingerprint(&tx, material_id)?;
        tx.execute(
            "UPDATE materials SET metadata_json=?2,metadata_fingerprint=?3,
             updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
            params![material_id, serde_json::to_string(metadata)?, fingerprint],
        )?;
        refresh_search(&tx, material_id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn search(&self, filter: &SearchFilter<'_>) -> Result<Vec<StoredMaterial>, CatalogError> {
        let text = filter.text.trim().to_lowercase();
        let has_text = !text.is_empty();
        let uses_fts = text.chars().count() >= 3;
        let text_clause = if text.is_empty() {
            "1"
        } else if uses_fts {
            "(m.id IN (SELECT rowid FROM material_fts WHERE material_fts MATCH ?1)
              OR instr(lower(substr(m.source_path,11)),?4)>0)"
        } else {
            "instr(lower(substr(m.source_path,11) || ' ' || m.search_text),?4)>0"
        };
        let query = if uses_fts {
            format!("\"{}\"", text.replace('"', "\"\""))
        } else {
            text.clone()
        };
        let sql = format!(
            "SELECT m.id,m.source_path,m.metadata_json,
                    (m.metadata_fingerprint IS NOT NULL AND m.metadata_fingerprint=m.fingerprint)
             FROM materials m WHERE {text_clause}
             AND (m.metadata_fingerprint IS NULL OR m.metadata_fingerprint=m.fingerprint)
             AND (?2 IS NULL OR m.id IN (SELECT material_id FROM material_attributes WHERE kind='tag' AND value=?2))
             AND (?3 IS NULL OR m.id IN (SELECT material_id FROM material_attributes WHERE kind='role' AND value=?3))
             ORDER BY m.source_path,m.id"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = if has_text {
            statement.query(params![query, filter.tag, filter.role, text])?
        } else {
            statement.query(params![query, filter.tag, filter.role])?
        };
        let mut materials = Vec::new();
        while let Some(row) = rows.next()? {
            let metadata_json: String = row.get(2)?;
            let current: bool = row.get(3)?;
            materials.push(StoredMaterial {
                id: row.get(0)?,
                source_path: row.get(1)?,
                metadata: serde_json::from_str(&metadata_json)?,
                availability: if !current && metadata_json != "{}" {
                    Availability::Stale
                } else {
                    Availability::Available
                },
            });
        }
        Ok(materials)
    }

    pub fn candidate_catalog(&self) -> Result<Catalog, CatalogError> {
        let materials = self
            .search(&SearchFilter::default())?
            .into_iter()
            .filter(|material| material.metadata != Metadata::default())
            .map(|material| CatalogMaterial {
                id: format!("material-{}", material.id),
                source_path: material.source_path,
                keywords: material.metadata.keywords,
                tags: material.metadata.tags,
                roles: material.metadata.roles,
                note: material.metadata.note,
            })
            .collect();
        Ok(Catalog {
            schema_version: SCHEMA_VERSION,
            material_root: self.root.to_string_lossy().into_owned(),
            materials,
        })
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(unc_path) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc_path}"));
        }
        if let Some(path) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(path);
        }
    }
    path
}

fn configure(connection: &Connection) -> Result<(), CatalogError> {
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}

fn materials_directory(root: &Path) -> Result<PathBuf, CatalogError> {
    let materials = root.join("materials");
    if materials.is_dir() {
        Ok(materials)
    } else {
        Err(CatalogError::MissingMaterialsDirectory(
            materials.display().to_string(),
        ))
    }
}

fn material_fingerprint(connection: &Connection, material_id: i64) -> Result<String, CatalogError> {
    connection
        .query_row(
            "SELECT fingerprint FROM materials WHERE id=?1",
            [material_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(CatalogError::NoMaterialSelected)
}

fn refresh_search(connection: &Connection, id: i64) -> Result<(), CatalogError> {
    let metadata_json: String = connection.query_row(
        "SELECT metadata_json FROM materials WHERE id=?1",
        [id],
        |row| row.get(0),
    )?;
    let metadata: Metadata = serde_json::from_str(&metadata_json)?;
    let search = format!(
        "{} {} {} {}",
        metadata.keywords.join(" "),
        metadata.tags.join(" "),
        metadata.roles.join(" "),
        metadata.note
    )
    .to_lowercase();
    connection.execute(
        "UPDATE materials SET search_text=?2 WHERE id=?1",
        params![id, search],
    )?;
    connection.execute("DELETE FROM material_attributes WHERE material_id=?1", [id])?;
    for (kind, values) in [("tag", &metadata.tags), ("role", &metadata.roles)] {
        for value in values {
            connection.execute(
                "INSERT OR IGNORE INTO material_attributes(material_id,kind,value) VALUES(?1,?2,?3)",
                params![id, kind, value],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_search_supports_short_filename_query() {
        let temp = tempfile::tempdir().unwrap();
        let materials = temp.path().join("materials");
        std::fs::create_dir(&materials).unwrap();
        std::fs::write(materials.join("01-tekkaman.wav"), b"fixture").unwrap();
        let path = temp.path().join("material-catalog.sqlite");
        let mut catalog = SqliteCatalog::create(&path, temp.path()).unwrap();
        catalog.rescan().unwrap();

        let materials = catalog
            .search(&SearchFilter {
                text: "te",
                ..Default::default()
            })
            .unwrap();

        assert_eq!(materials.len(), 1);
        assert_eq!(materials[0].source_path, "materials/01-tekkaman.wav");
    }

    #[test]
    fn rescan_keeps_metadata_for_an_unambiguous_rename_and_discards_deleted_file() {
        let temp = tempfile::tempdir().unwrap();
        let materials = temp.path().join("materials");
        std::fs::create_dir(&materials).unwrap();
        let original = materials.join("01-tekkaman.wav");
        let renamed = materials.join("01-tekkaman-renamed.wav");
        std::fs::write(&original, b"fixture").unwrap();

        let path = temp.path().join("material-catalog.sqlite");
        let mut catalog = SqliteCatalog::create(&path, temp.path()).unwrap();
        catalog.rescan().unwrap();
        let material_id = catalog.materials().unwrap()[0].id;
        catalog
            .save_metadata(
                material_id,
                &Metadata {
                    keywords: vec!["テッカマン".into()],
                    tags: vec!["名前".into()],
                    ..Default::default()
                },
            )
            .unwrap();

        std::fs::rename(&original, &renamed).unwrap();
        let report = catalog.rescan().unwrap();
        assert_eq!(report.moved, 1);
        assert_eq!(report.added, 0);
        assert_eq!(report.removed, 0);
        assert_eq!(catalog.materials().unwrap().len(), 1);
        assert_eq!(
            catalog.materials().unwrap()[0].source_path,
            "materials/01-tekkaman-renamed.wav"
        );
        assert_eq!(
            catalog
                .search(&SearchFilter {
                    text: "テッカマン",
                    ..Default::default()
                })
                .unwrap()[0]
                .metadata
                .tags,
            vec!["名前"]
        );

        std::fs::rename(&renamed, &original).unwrap();
        let report = catalog.rescan().unwrap();
        assert_eq!(report.moved, 1);
        assert_eq!(report.added, 0);
        assert_eq!(report.removed, 0);
        assert_eq!(catalog.materials().unwrap().len(), 1);
        assert_eq!(
            catalog.materials().unwrap()[0].source_path,
            "materials/01-tekkaman.wav"
        );

        std::fs::remove_file(&original).unwrap();
        let report = catalog.rescan().unwrap();
        assert_eq!(report.removed, 1);
        assert!(catalog.materials().unwrap().is_empty());
    }
}
