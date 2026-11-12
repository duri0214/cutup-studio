use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Metadata {
    pub keywords: Vec<String>,
    pub tags: Vec<String>,
    pub roles: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    Stale,
}

impl Availability {
    pub fn label(self) -> &'static str {
        match self {
            Self::Available => "利用可能",
            Self::Stale => "要確認（素材変更）",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Material {
    pub id: i64,
    pub source_path: String,
    pub fingerprint: String,
}

/// An analysis result remains separate from the user's metadata until reviewed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisCandidate {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub model: String,
    pub model_version: String,
    pub confidence: Option<f64>,
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisRecord {
    pub fingerprint: String,
    pub configuration: String,
    pub status: AnalysisStatus,
    pub candidates: Vec<AnalysisCandidate>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone)]
pub struct StoredMaterial {
    pub id: i64,
    pub source_path: String,
    pub metadata: Metadata,
    pub availability: Availability,
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub added: usize,
    pub changed: usize,
    pub moved: usize,
    pub removed: usize,
}

#[derive(Default)]
pub struct SearchFilter<'a> {
    pub text: &'a str,
    pub tag: Option<&'a str>,
    pub role: Option<&'a str>,
}
