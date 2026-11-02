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
