use super::material_scan;
use crate::domain::valueobject::{Catalog, SCHEMA_VERSION, Segment, catalog::*};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use thiserror::Error;

const APPLICATION_ID: i64 = 1129665603;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("カタログの読み書きに失敗しました: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("素材または保存先にアクセスできません: {0}")]
    Io(#[from] std::io::Error),
    #[error("メタデータを読み書きできません: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Metadata(#[from] MetadataError),
    #[error("対応していないカタログ形式またはバージョンです")]
    UnsupportedSchema,
    #[error("素材ルート外への参照は登録できません")]
    OutsideRoot,
    #[error("素材パスは UTF-8 で表せる実在のフォルダを指定してください")]
    InvalidPath,
    #[error("走査中に素材が変更されました。再走査してください")]
    FileChanged,
    #[error("素材または区間が利用できません。再走査して確認してください")]
    Unavailable,
    #[error("解析ジョブが実行中ではないか、入力素材が変更されています")]
    InvalidJob,
}

pub struct SqliteCatalog {
    connection: Connection,
    root: PathBuf,
}

impl SqliteCatalog {
    /// Existing files are never overwritten, including unrelated SQLite databases.
    pub fn create(path: &Path, root: &Path) -> Result<Self, CatalogError> {
        let root = normalize_path(root.canonicalize()?);
        if !root.is_dir() {
            return Err(CatalogError::InvalidPath);
        }
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
        if app != APPLICATION_ID || version != 1 {
            return Err(CatalogError::UnsupportedSchema);
        }
        let root: String =
            connection.query_row("SELECT material_root FROM catalog WHERE id=1", [], |row| {
                row.get(0)
            })?;
        Ok(Self {
            connection,
            root: normalize_path(root.into()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rescan(&mut self) -> Result<ScanReport, CatalogError> {
        // Finish all filesystem reads before changing availability; a failed scan is atomic.
        let files = material_scan::scan(&self.root)?;
        let old: BTreeMap<_, _> = self
            .materials()?
            .into_iter()
            .map(|m| (m.source_path.clone(), m))
            .collect();
        let tx = self.connection.transaction()?;
        let mut report = ScanReport::default();
        let mut remaining = old;
        for file in files {
            let previous = remaining.remove(&file.path);
            match &previous {
                None => report.added += 1,
                Some(m) => {
                    if m.fingerprint != file.fingerprint {
                        report.changed += 1;
                    }
                    if !m.available {
                        report.restored += 1;
                    }
                }
            }
            tx.execute(
                "INSERT INTO materials(source_path,fingerprint,size_bytes) VALUES(?1,?2,?3)
                 ON CONFLICT(source_path) DO UPDATE SET
                 needs_analysis=CASE WHEN materials.fingerprint<>excluded.fingerprint THEN 1 ELSE materials.needs_analysis END,
                 fingerprint=excluded.fingerprint,size_bytes=excluded.size_bytes,available=1,
                 updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')",
                params![file.path, file.fingerprint, file.size],
            )?;
        }
        for material in remaining.values().filter(|m| m.available) {
            tx.execute("UPDATE materials SET available=0,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [material.id])?;
            report.missing += 1;
        }
        tx.execute("UPDATE analysis_jobs SET status='superseded',finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
                    WHERE status='running' AND EXISTS(SELECT 1 FROM materials m WHERE m.id=material_id
                    AND (m.available=0 OR m.fingerprint<>input_fingerprint))", [])?;
        tx.commit()?;
        Ok(report)
    }

    pub fn materials(&self) -> Result<Vec<Material>, CatalogError> {
        let mut statement = self.connection.prepare(
            "SELECT id,source_path,fingerprint,available FROM materials ORDER BY source_path",
        )?;
        Ok(statement
            .query_map([], |row| {
                Ok(Material {
                    id: row.get(0)?,
                    source_path: row.get(1)?,
                    fingerprint: row.get(2)?,
                    available: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?)
    }

    pub fn analysis_targets(&self) -> Result<Vec<i64>, CatalogError> {
        let mut statement = self.connection.prepare(
            "SELECT id FROM materials WHERE available=1 AND needs_analysis=1 ORDER BY id",
        )?;
        Ok(statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?)
    }

    /// Starts an explicitly requested local analysis; scanning does not run a model.
    pub fn start_analysis(
        &mut self,
        material_id: i64,
        model: &str,
        version: &str,
    ) -> Result<i64, CatalogError> {
        if model.trim().is_empty() || version.trim().is_empty() {
            return Err(MetadataError::InvalidProvenance.into());
        }
        let tx = self.connection.transaction()?;
        let fingerprint = available_fingerprint(&tx, material_id)?;
        tx.execute("INSERT INTO analysis_jobs(material_id,input_fingerprint,model,version,status) VALUES(?1,?2,?3,?4,'running')",
            params![material_id, fingerprint, model, version])?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE materials SET needs_analysis=1 WHERE id=?1",
            [material_id],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn fail_analysis(&mut self, job_id: i64, reason: &str) -> Result<(), CatalogError> {
        if self.connection.execute("UPDATE analysis_jobs SET status='failed',error=?2,finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND status='running'", params![job_id,reason])? != 1 {
            return Err(CatalogError::InvalidJob);
        }
        Ok(())
    }

    pub fn complete_analysis(
        &mut self,
        job_id: i64,
        results: &[AnalysisResult],
    ) -> Result<(), CatalogError> {
        for result in results {
            result.validate()?;
        }
        let tx = self.connection.transaction()?;
        let (material_id, fingerprint): (i64, String) = tx.query_row(
            "SELECT j.material_id,j.input_fingerprint FROM analysis_jobs j JOIN materials m ON m.id=j.material_id
             WHERE j.id=?1 AND j.status='running' AND m.available=1 AND m.fingerprint=j.input_fingerprint",
            [job_id], |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional()?.ok_or(CatalogError::InvalidJob)?;
        // Keep historical ranges for inspection, but retire old AI-only ranges from candidates.
        tx.execute(
            "UPDATE segments SET active=0 WHERE material_id=?1 AND manual_updated_at IS NULL",
            [material_id],
        )?;
        for result in results {
            let ai = serde_json::to_string(&result.metadata)?;
            tx.execute("INSERT INTO segments(material_id,start_ms,end_ms,input_fingerprint,ai_json,job_id)
                        VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(material_id,start_ms,end_ms) DO UPDATE SET
                        ai_json=excluded.ai_json,job_id=excluded.job_id,active=1,input_fingerprint=excluded.input_fingerprint,
                        updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')",
                params![material_id,result.range.start_ms(),result.range.end_ms(),fingerprint,ai,job_id])?;
            let id: i64 = tx.query_row(
                "SELECT id FROM segments WHERE material_id=?1 AND start_ms=?2 AND end_ms=?3",
                params![material_id, result.range.start_ms(), result.range.end_ms()],
                |row| row.get(0),
            )?;
            tx.execute("INSERT INTO analysis_results(job_id,segment_id,metadata_json,confidence,evidence,start_ms,end_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![job_id,id,ai,result.confidence,result.evidence,result.range.start_ms(),result.range.end_ms()])?;
            refresh_search(&tx, id)?;
        }
        tx.execute(
            "UPDATE materials SET needs_analysis=0 WHERE id=?1",
            [material_id],
        )?;
        tx.execute("UPDATE analysis_jobs SET status='completed',finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [job_id])?;
        tx.commit()?;
        Ok(())
    }

    /// Saving an existing ID preserves its identity, including when the user corrects its range.
    pub fn save_manual(
        &mut self,
        id: Option<i64>,
        material_id: i64,
        range: TimeRange,
        manual: &ManualMetadata,
    ) -> Result<i64, CatalogError> {
        let tx = self.connection.transaction()?;
        let fingerprint = available_fingerprint(&tx, material_id)?;
        let manual_json = serde_json::to_string(manual)?;
        let id = if let Some(id) = id {
            let count = tx.execute("UPDATE segments SET start_ms=?2,end_ms=?3,manual_json=?4,input_fingerprint=?5,active=1,
                                  manual_updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                                  updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND material_id=?6",
                params![id,range.start_ms(),range.end_ms(),manual_json,fingerprint,material_id])?;
            if count != 1 {
                return Err(CatalogError::Unavailable);
            }
            id
        } else {
            tx.execute("INSERT INTO segments(material_id,start_ms,end_ms,input_fingerprint,manual_json,manual_updated_at)
                        VALUES(?1,?2,?3,?4,?5,strftime('%Y-%m-%dT%H:%M:%fZ','now'))
                        ON CONFLICT(material_id,start_ms,end_ms) DO UPDATE SET manual_json=excluded.manual_json,
                        manual_updated_at=excluded.manual_updated_at,input_fingerprint=excluded.input_fingerprint,
                        active=1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')",
                params![material_id,range.start_ms(),range.end_ms(),fingerprint,manual_json])?;
            tx.query_row(
                "SELECT id FROM segments WHERE material_id=?1 AND start_ms=?2 AND end_ms=?3",
                params![material_id, range.start_ms(), range.end_ms()],
                |row| row.get(0),
            )?
        };
        refresh_search(&tx, id)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn search(&self, filter: &SearchFilter<'_>) -> Result<Vec<StoredSegment>, CatalogError> {
        let text = filter.text.trim().to_lowercase();
        let text_clause = if text.is_empty() {
            "1"
        } else if text.chars().count() >= 3 {
            "s.id IN (SELECT rowid FROM segment_fts WHERE segment_fts MATCH ?1)"
        } else {
            "instr(s.search_text,?1)>0"
        };
        let query = if text.chars().count() >= 3 {
            format!("\"{}\"", text.replace('"', "\"\""))
        } else {
            text
        };
        let sql = format!("SELECT s.id,s.material_id,m.source_path,s.start_ms,s.end_ms,s.effective_json,s.manual_json,
                           m.available,(s.active=1 AND s.input_fingerprint=m.fingerprint)
                           FROM segments s JOIN materials m ON m.id=s.material_id WHERE {text_clause}
                           AND (?2 OR (m.available=1 AND s.active=1 AND s.input_fingerprint=m.fingerprint))
                           AND (?3 IS NULL OR s.id IN (SELECT segment_id FROM segment_attributes WHERE kind='tag' AND value=?3))
                           AND (?4 IS NULL OR s.id IN (SELECT segment_id FROM segment_attributes WHERE kind='role' AND value=?4))
                           ORDER BY m.source_path,s.start_ms,s.id");
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params![
            query,
            filter.include_unavailable,
            filter.tag,
            filter.role
        ])?;
        let mut segments = Vec::new();
        while let Some(row) = rows.next()? {
            let available: bool = row.get(7)?;
            let current: bool = row.get(8)?;
            segments.push(StoredSegment {
                id: row.get(0)?,
                material_id: row.get(1)?,
                source_path: row.get(2)?,
                range: TimeRange::new(row.get(3)?, row.get(4)?)?,
                metadata: serde_json::from_str(&row.get::<_, String>(5)?)?,
                manual: serde_json::from_str(&row.get::<_, String>(6)?)?,
                availability: if !available {
                    Availability::Missing
                } else if !current {
                    Availability::Stale
                } else {
                    Availability::Available
                },
            });
        }
        Ok(segments)
    }

    pub fn candidate_catalog(&self) -> Result<Catalog, CatalogError> {
        let segments = self
            .search(&SearchFilter::default())?
            .into_iter()
            .map(|s| {
                let mut tags = s.metadata.tags;
                tags.extend(s.metadata.visual_tags);
                tags.extend(s.metadata.audio_tags);
                Segment {
                    id: format!("segment-{}", s.id),
                    source_path: s.source_path,
                    start_ms: s.range.start_ms() as u64,
                    end_ms: s.range.end_ms() as u64,
                    transcript: s.metadata.transcript,
                    keywords: s.metadata.keywords,
                    tags,
                    roles: s.metadata.roles,
                    note: s.metadata.note,
                }
            })
            .collect();
        Ok(Catalog {
            schema_version: SCHEMA_VERSION,
            material_root: self.root.to_string_lossy().into_owned(),
            segments,
        })
    }

    pub fn analysis_history(&self, segment_id: i64) -> Result<Vec<AnalysisRecord>, CatalogError> {
        let mut statement = self.connection.prepare("SELECT j.model,j.version,j.input_fingerprint,r.confidence,r.evidence,j.created_at,
            r.start_ms,r.end_ms,r.metadata_json
            FROM analysis_results r JOIN analysis_jobs j ON j.id=r.job_id WHERE r.segment_id=?1 ORDER BY j.id DESC")?;
        let mut rows = statement.query([segment_id])?;
        let mut history = Vec::new();
        while let Some(row) = rows.next()? {
            history.push(AnalysisRecord {
                model: row.get(0)?,
                version: row.get(1)?,
                input_fingerprint: row.get(2)?,
                confidence: row.get(3)?,
                evidence: row.get(4)?,
                created_at: row.get(5)?,
                range: TimeRange::new(row.get(6)?, row.get(7)?)?,
                metadata: serde_json::from_str(&row.get::<_, String>(8)?)?,
            });
        }
        Ok(history)
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

fn available_fingerprint(
    connection: &Connection,
    material_id: i64,
) -> Result<String, CatalogError> {
    connection
        .query_row(
            "SELECT fingerprint FROM materials WHERE id=?1 AND available=1",
            [material_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(CatalogError::Unavailable)
}

fn refresh_search(connection: &Connection, id: i64) -> Result<(), CatalogError> {
    let (ai, manual): (String, String) = connection.query_row(
        "SELECT ai_json,manual_json FROM segments WHERE id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let metadata =
        serde_json::from_str::<ManualMetadata>(&manual)?.resolve(&serde_json::from_str(&ai)?);
    let search = format!(
        "{} {} {} {} {} {} {} {}",
        metadata.transcript,
        metadata.speakers.join(" "),
        metadata.keywords.join(" "),
        metadata.visual_tags.join(" "),
        metadata.audio_tags.join(" "),
        metadata.tags.join(" "),
        metadata.roles.join(" "),
        metadata.note
    )
    .to_lowercase();
    connection.execute(
        "UPDATE segments SET effective_json=?2,search_text=?3 WHERE id=?1",
        params![id, serde_json::to_string(&metadata)?, search],
    )?;
    connection.execute("DELETE FROM segment_attributes WHERE segment_id=?1", [id])?;
    for (kind, values) in [
        ("tag", &metadata.tags),
        ("tag", &metadata.visual_tags),
        ("tag", &metadata.audio_tags),
        ("role", &metadata.roles),
        ("speaker", &metadata.speakers),
    ] {
        for value in values {
            connection.execute(
                "INSERT OR IGNORE INTO segment_attributes(segment_id,kind,value) VALUES(?1,?2,?3)",
                params![id, kind, value],
            )?;
        }
    }
    Ok(())
}
