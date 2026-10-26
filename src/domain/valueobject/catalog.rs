use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("開始・終了時刻は 0 <= 開始 < 終了 のミリ秒で指定してください")]
    InvalidRange,
    #[error("解析のモデル名・バージョン・根拠と、0〜1 の確度が必要です")]
    InvalidProvenance,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "RawTimeRange")]
pub struct TimeRange {
    start_ms: i64,
    end_ms: i64,
}

#[derive(Deserialize)]
struct RawTimeRange {
    start_ms: i64,
    end_ms: i64,
}

impl TryFrom<RawTimeRange> for TimeRange {
    type Error = MetadataError;
    fn try_from(value: RawTimeRange) -> Result<Self, Self::Error> {
        Self::new(value.start_ms, value.end_ms)
    }
}

impl TimeRange {
    pub fn new(start_ms: i64, end_ms: i64) -> Result<Self, MetadataError> {
        if start_ms < 0 || end_ms <= start_ms {
            return Err(MetadataError::InvalidRange);
        }
        Ok(Self { start_ms, end_ms })
    }

    pub fn start_ms(self) -> i64 {
        self.start_ms
    }
    pub fn end_ms(self) -> i64 {
        self.end_ms
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Metadata {
    pub transcript: String,
    pub speakers: Vec<String>,
    pub keywords: Vec<String>,
    pub visual_tags: Vec<String>,
    pub audio_tags: Vec<String>,
    pub tags: Vec<String>,
    pub roles: Vec<String>,
    pub note: String,
}

/// None は AI 値を使用、Some(空値) は利用者による明示的な削除。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ManualMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speakers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl ManualMetadata {
    pub fn resolve(&self, ai: &Metadata) -> Metadata {
        Metadata {
            transcript: self.transcript.as_ref().unwrap_or(&ai.transcript).clone(),
            speakers: self.speakers.as_ref().unwrap_or(&ai.speakers).clone(),
            keywords: self.keywords.as_ref().unwrap_or(&ai.keywords).clone(),
            visual_tags: self.visual_tags.as_ref().unwrap_or(&ai.visual_tags).clone(),
            audio_tags: self.audio_tags.as_ref().unwrap_or(&ai.audio_tags).clone(),
            tags: self.tags.as_ref().unwrap_or(&ai.tags).clone(),
            roles: self.roles.as_ref().unwrap_or(&ai.roles).clone(),
            note: self.note.as_ref().unwrap_or(&ai.note).clone(),
        }
    }
}

impl From<Metadata> for ManualMetadata {
    fn from(value: Metadata) -> Self {
        Self {
            transcript: Some(value.transcript),
            speakers: Some(value.speakers),
            keywords: Some(value.keywords),
            visual_tags: Some(value.visual_tags),
            audio_tags: Some(value.audio_tags),
            tags: Some(value.tags),
            roles: Some(value.roles),
            note: Some(value.note),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub range: TimeRange,
    pub metadata: Metadata,
    pub confidence: f64,
    pub evidence: String,
}

impl AnalysisResult {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if !self.confidence.is_finite()
            || !(0.0..=1.0).contains(&self.confidence)
            || self.evidence.trim().is_empty()
        {
            return Err(MetadataError::InvalidProvenance);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    Missing,
    Stale,
}

impl Availability {
    pub fn label(self) -> &'static str {
        match self {
            Self::Available => "利用可能",
            Self::Missing => "利用不能（移動・削除）",
            Self::Stale => "要確認（素材変更・旧解析区間）",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Material {
    pub id: i64,
    pub source_path: String,
    pub fingerprint: String,
    pub available: bool,
}

#[derive(Debug, Clone)]
pub struct StoredSegment {
    pub id: i64,
    pub material_id: i64,
    pub source_path: String,
    pub range: TimeRange,
    pub metadata: Metadata,
    pub manual: ManualMetadata,
    pub availability: Availability,
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub added: usize,
    pub changed: usize,
    pub missing: usize,
    pub restored: usize,
}

#[derive(Debug)]
pub struct AnalysisRecord {
    pub model: String,
    pub version: String,
    pub input_fingerprint: String,
    pub confidence: f64,
    pub evidence: String,
    pub created_at: String,
    pub range: TimeRange,
    pub metadata: Metadata,
}

#[derive(Default)]
pub struct SearchFilter<'a> {
    pub text: &'a str,
    pub tag: Option<&'a str>,
    pub role: Option<&'a str>,
    pub include_unavailable: bool,
}
