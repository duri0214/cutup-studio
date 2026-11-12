//! Local speech-to-text using FFmpeg and whisper.cpp.
use crate::domain::valueobject::catalog::AnalysisCandidate;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SttError {
    #[error("{0} を実行できません。実行ファイルと設定を確認してください: {1}")]
    Start(&'static str, std::io::Error),
    #[error("{0} が失敗しました。素材、モデル、ツールの設定を確認してください")]
    Failed(&'static str),
    #[error("音声認識結果を読み取れません: {0}")]
    Json(#[from] serde_json::Error),
    #[error("一時ファイルまたはモデルを読み取れません: {0}")]
    Io(#[from] std::io::Error),
    #[error("音声認識結果の区間が不正です")]
    InvalidSegment,
    #[error("解析を中断しました")]
    Interrupted,
    #[error("素材が変更されました。基準フォルダを開き直してから再解析してください")]
    MaterialChanged,
}

pub struct SttConfig {
    pub ffmpeg: PathBuf,
    pub whisper: PathBuf,
    pub model: PathBuf,
}

#[derive(Deserialize)]
struct Transcript {
    transcription: Vec<Segment>,
    result: Option<TranscriptResult>,
}
#[derive(Deserialize)]
struct TranscriptResult {
    language: Option<String>,
}
#[derive(Deserialize)]
struct Segment {
    text: String,
    offsets: Offsets,
}
#[derive(Deserialize)]
struct Offsets {
    from: u64,
    to: u64,
}

pub fn transcribe(
    config: &SttConfig,
    input: &Path,
    expected_fingerprint: &str,
    cancel: &AtomicBool,
) -> Result<Vec<AnalysisCandidate>, SttError> {
    if fingerprint(input, cancel)? != expected_fingerprint {
        return Err(SttError::MaterialChanged);
    }
    let mut model = fs::File::open(&config.model)?;
    let mut hash = Sha256::new();
    let mut bytes = [0_u8; 65536];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(SttError::Interrupted);
        }
        let n = model.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    let version = format!("sha256:{:x}", hash.finalize());
    let model_name = config
        .model
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("whisper model");
    let temp = tempfile::tempdir()?;
    let audio = temp.path().join("audio.wav");
    let output = temp.path().join("transcript");
    let mut ffmpeg = Command::new(&config.ffmpeg);
    ffmpeg
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", "-y"])
        .arg(&audio)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    run(&mut ffmpeg, "FFmpeg", cancel)?;
    let mut whisper = Command::new(&config.whisper);
    whisper
        .args(["-m"])
        .arg(&config.model)
        .args(["-l", "auto", "-oj", "-of"])
        .arg(&output)
        .arg("-f")
        .arg(&audio)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    run(&mut whisper, "whisper-cli", cancel)?;
    if fingerprint(input, cancel)? != expected_fingerprint {
        return Err(SttError::MaterialChanged);
    }
    parse_transcript(
        &fs::read(output.with_extension("json"))?,
        model_name,
        &version,
    )
}

fn fingerprint(path: &Path, cancel: &AtomicBool) -> Result<String, SttError> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(SttError::Interrupted);
        }
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn run(command: &mut Command, name: &'static str, cancel: &AtomicBool) -> Result<(), SttError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(SttError::Interrupted);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|e| SttError::Start(name, e))?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SttError::Interrupted);
        }
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(SttError::Failed(name))
            };
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn parse_transcript(
    json: &[u8],
    model: &str,
    version: &str,
) -> Result<Vec<AnalysisCandidate>, SttError> {
    let transcript: Transcript = serde_json::from_slice(json)?;
    let language = transcript.result.and_then(|result| result.language);
    transcript
        .transcription
        .into_iter()
        .map(|segment| {
            if segment.offsets.from > segment.offsets.to {
                return Err(SttError::InvalidSegment);
            }
            Ok(AnalysisCandidate {
                original_text: Some(segment.text.clone()),
                language: language.clone(),
                text: segment.text,
                start_ms: segment.offsets.from,
                end_ms: segment.offsets.to,
                model: model.into(),
                model_version: version.into(),
                confidence: None,
                evidence: Some("音声からの文字起こし候補".into()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_ffmpeg_reports_which_tool_needs_installation() {
        let temp = tempfile::tempdir().unwrap();
        let model = temp.path().join("model.bin");
        let media = temp.path().join("media.wav");
        fs::write(&model, b"model").unwrap();
        fs::write(&media, b"audio").unwrap();
        let cancel = AtomicBool::new(false);
        let config = SttConfig {
            ffmpeg: temp.path().join("missing-ffmpeg"),
            whisper: temp.path().join("whisper"),
            model,
        };
        let digest = fingerprint(&media, &cancel).unwrap();
        assert!(matches!(
            transcribe(&config, &media, &digest, &cancel),
            Err(SttError::Start("FFmpeg", _))
        ));
    }

    #[test]
    fn changed_material_is_rejected_before_tools_are_started() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media.wav");
        fs::write(&media, b"changed").unwrap();
        let config = SttConfig {
            ffmpeg: temp.path().join("missing-ffmpeg"),
            whisper: temp.path().join("missing-whisper"),
            model: temp.path().join("missing-model"),
        };
        assert!(matches!(
            transcribe(&config, &media, "old-fingerprint", &AtomicBool::new(false)),
            Err(SttError::MaterialChanged)
        ));
    }
    #[test]
    #[ignore = "requires local ffmpeg, whisper-cli and a model"]
    fn real_tools_transcribe_demo_material() {
        let config = SttConfig {
            ffmpeg: std::env::var_os("CUTUP_TEST_FFMPEG")
                .expect("set CUTUP_TEST_FFMPEG")
                .into(),
            whisper: std::env::var_os("CUTUP_TEST_WHISPER")
                .expect("set CUTUP_TEST_WHISPER")
                .into(),
            model: std::env::var_os("CUTUP_TEST_MODEL")
                .expect("set CUTUP_TEST_MODEL")
                .into(),
        };
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/materials/08-nani.wav");
        let cancel = AtomicBool::new(false);
        let digest = fingerprint(&source, &cancel).unwrap();
        let mut candidates = transcribe(&config, &source, &digest, &cancel).unwrap();
        assert!(!candidates.is_empty());
        assert!(
            candidates
                .iter()
                .any(|candidate| !candidate.text.trim().is_empty())
        );
        assert!(
            candidates
                .iter()
                .all(|c| c.end_ms >= c.start_ms && !c.model_version.is_empty())
        );
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("materials")).unwrap();
        fs::copy(&source, temp.path().join("materials/sample.wav")).unwrap();
        let path = temp.path().join("catalog.sqlite");
        let mut store =
            crate::infra::sqlite_catalog::SqliteCatalog::create(&path, temp.path()).unwrap();
        store.rescan().unwrap();
        let material = store.materials().unwrap().remove(0);
        candidates[0].text = "確認後の修正".into();
        let record = crate::domain::valueobject::catalog::AnalysisRecord {
            fingerprint: material.fingerprint,
            configuration: "実環境テスト".into(),
            status: crate::domain::valueobject::catalog::AnalysisStatus::Completed,
            candidates,
            error: None,
        };
        store.save_analysis(material.id, &record).unwrap();
        drop(store);
        let reopened = crate::infra::sqlite_catalog::SqliteCatalog::open(&path).unwrap();
        assert_eq!(
            reopened.analysis(material.id).unwrap().unwrap().candidates[0].text,
            "確認後の修正"
        );
    }
    #[test]
    fn cancellation_before_start_does_not_launch_a_process() {
        let cancelled = AtomicBool::new(true);
        let mut nonexistent = Command::new("cutup-nonexistent-ffmpeg");
        assert!(matches!(
            run(&mut nonexistent, "FFmpeg", &cancelled),
            Err(SttError::Interrupted)
        ));
    }
    #[test]
    fn parses_whisper_segments_with_timing_and_rejects_reversed_ranges() {
        let candidates = parse_transcript(
            br#"{"transcription":[{"text":"hello","offsets":{"from":10,"to":900}}]}"#,
            "base",
            "hash",
        )
        .unwrap();
        assert_eq!(
            (
                candidates[0].start_ms,
                candidates[0].end_ms,
                candidates[0].text.as_str()
            ),
            (10, 900, "hello")
        );
        assert!(matches!(
            parse_transcript(
                br#"{"transcription":[{"text":"x","offsets":{"from":9,"to":1}}]}"#,
                "base",
                "hash"
            ),
            Err(SttError::InvalidSegment)
        ));
    }
}
