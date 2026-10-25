use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Component, Path};
use thiserror::Error;

pub const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("素材 {id} の source_path は素材ルートからの相対パスで指定してください: {path}")]
    UnsafeSourcePath { id: String, path: String },
    #[error("素材 {id} の時間範囲が不正です")]
    InvalidRange { id: String },
    #[error("素材 ID が重複しています: {0}")]
    DuplicateId(String),
    #[error("スロット `{0}` の選択がありません")]
    MissingSelection(String),
    #[error("スロット `{slot}` に候補 `{candidate}` はありません")]
    CandidateNotFound { slot: String, candidate: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Catalog {
    pub schema_version: u8,
    pub material_root: String,
    pub segments: Vec<Segment>,
}

impl Catalog {
    pub fn validate_metadata(&self) -> Result<(), DomainError> {
        let mut ids = BTreeSet::new();
        for segment in &self.segments {
            if !ids.insert(&segment.id) {
                return Err(DomainError::DuplicateId(segment.id.clone()));
            }
            segment.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Segment {
    pub id: String,
    /// `material_root` からの相対パス。素材本体はアプリへコピーしない。
    pub source_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default)]
    pub transcript: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub note: String,
}

impl Segment {
    fn validate(&self) -> Result<(), DomainError> {
        let path = Path::new(&self.source_path);
        if path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::Prefix(_) | Component::RootDir
                )
            })
        {
            return Err(DomainError::UnsafeSourcePath {
                id: self.id.clone(),
                path: self.source_path.clone(),
            });
        }
        if self.end_ms <= self.start_ms {
            return Err(DomainError::InvalidRange {
                id: self.id.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GuideSlot {
    pub id: String,
    pub order: usize,
    pub role: String,
    pub query: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Candidate {
    pub segment_id: String,
    pub score: u32,
    pub reasons: Vec<String>,
    pub source_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub transcript: String,
    pub tags: Vec<String>,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlotPlan {
    #[serde(flatten)]
    pub slot: GuideSlot,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Plan {
    pub schema_version: u8,
    pub guide_text: String,
    pub material_root: String,
    pub slots: Vec<SlotPlan>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub schema_version: u8,
    pub guide_text: String,
    pub material_root: String,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestEntry {
    pub order: usize,
    pub slot_id: String,
    pub role: String,
    pub query: String,
    pub segment_id: String,
    pub source_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub selection_reason: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_rejects_a_source_path_outside_the_material_root() {
        let catalog = Catalog {
            schema_version: 1,
            material_root: "C:/media/cutup-samples".into(),
            segments: vec![Segment {
                id: "outside".into(),
                source_path: "../outside.mp4".into(),
                start_ms: 0,
                end_ms: 100,
                transcript: String::new(),
                keywords: Vec::new(),
                tags: Vec::new(),
                roles: Vec::new(),
                note: String::new(),
            }],
        };

        assert!(matches!(
            catalog.validate_metadata(),
            Err(DomainError::UnsafeSourcePath { .. })
        ));
    }
}
