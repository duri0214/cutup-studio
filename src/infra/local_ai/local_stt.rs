//! Local speech-to-text using FFmpeg and whisper.cpp.
use crate::domain::valueobject::catalog::AnalysisCandidate;
use lindera::segmenter::Segmenter;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
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
    #[error("日本語の形態素解析に失敗しました: {0}")]
    Morphology(String),
    #[error("解析を中断しました")]
    Interrupted,
    #[error("素材が変更されました。基準フォルダを開き直してから再解析してください")]
    MaterialChanged,
}

pub struct SttConfig {
    pub ffmpeg: PathBuf,
    pub whisper: PathBuf,
    pub model: PathBuf,
    pub language: SpeechLanguage,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeechLanguage {
    #[default]
    Auto,
    Japanese,
}

impl SpeechLanguage {
    pub fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Japanese => "ja",
        }
    }
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
struct WordTranscript {
    transcription: Vec<WordSegment>,
    result: Option<TranscriptResult>,
}
#[derive(Deserialize)]
struct WordSegment {
    text: String,
    offsets: Offsets,
    #[serde(default)]
    tokens: Option<Vec<WhisperToken>>,
}
#[derive(Deserialize)]
struct WhisperToken {
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
    transcribe_with_granularity(config, input, expected_fingerprint, cancel, false)
}

pub fn transcribe_for_splitting(
    config: &SttConfig,
    input: &Path,
    expected_fingerprint: &str,
    cancel: &AtomicBool,
) -> Result<Vec<AnalysisCandidate>, SttError> {
    transcribe_with_granularity(config, input, expected_fingerprint, cancel, true)
}

fn transcribe_with_granularity(
    config: &SttConfig,
    input: &Path,
    expected_fingerprint: &str,
    cancel: &AtomicBool,
    short_segments: bool,
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
        .args(["-l", config.language.code()])
        .arg(if short_segments && dtw_model(&config.model).is_some() {
            "-ojf"
        } else {
            "-oj"
        })
        .arg("-of")
        .arg(&output)
        .arg("-f")
        .arg(&audio)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if short_segments {
        whisper.args(["-ml", "15"]);
        if let Some(model) = dtw_model(&config.model) {
            whisper.args(["-dtw", model]);
        }
    }
    run(&mut whisper, "whisper-cli", cancel)?;
    if fingerprint(input, cancel)? != expected_fingerprint {
        return Err(SttError::MaterialChanged);
    }
    let json = fs::read(output.with_extension("json"))?;
    let result = if short_segments && dtw_model(&config.model).is_some() {
        parse_word_transcript(&json, model_name, &version)
    } else {
        parse_transcript(&json, model_name, &version)
    };
    match result {
        Err(SttError::Json(_)) if short_segments => {
            transcribe_with_granularity(config, input, expected_fingerprint, cancel, false)
        }
        other => other,
    }
}

fn dtw_model(model_path: &Path) -> Option<&'static str> {
    let stem = model_path
        .file_stem()?
        .to_str()?
        .to_ascii_lowercase()
        .strip_prefix("ggml-")?
        .to_owned();
    [
        "large-v3-turbo",
        "large-v3",
        "large-v2",
        "large-v1",
        "medium.en",
        "medium",
        "small.en",
        "small",
        "base.en",
        "base",
        "tiny.en",
        "tiny",
        "turbo",
    ]
    .into_iter()
    .find(|preset| {
        stem == *preset
            || stem.strip_prefix(*preset).is_some_and(|suffix| {
                suffix.starts_with('-') || suffix.starts_with('.') || suffix.starts_with('_')
            })
    })
}

pub fn fingerprint(path: &Path, cancel: &AtomicBool) -> Result<String, SttError> {
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

fn parse_word_transcript(
    json: &[u8],
    model: &str,
    version: &str,
) -> Result<Vec<AnalysisCandidate>, SttError> {
    let json = String::from_utf8_lossy(json);
    let transcript: WordTranscript = serde_json::from_str(&json)?;
    let language = transcript.result.and_then(|result| result.language);
    let config = serde_json::json!({
        "dictionary": "embedded://ipadic",
        "mode": "normal",
    });
    let mut segmenter = Segmenter::from_config(&config)
        .map_err(|error| SttError::Morphology(error.to_string()))?
        .into_worker();
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    for segment in transcript.transcription {
        if segment.offsets.from > segment.offsets.to {
            return Err(SttError::InvalidSegment);
        }
        if segment.text.trim().is_empty() {
            continue;
        }
        let morphemes = segmenter
            .segment(&segment.text)
            .map_err(|error| SttError::Morphology(error.to_string()))?;
        let Some(character_times) = segment
            .tokens
            .as_deref()
            .and_then(|tokens| align_character_times(&segment.text, tokens))
        else {
            push_candidate_once(
                segment_candidate(
                    &segment.text,
                    segment.offsets.from,
                    segment.offsets.to,
                    language.as_deref(),
                    model,
                    version,
                ),
                &mut seen,
                &mut candidates,
            );
            continue;
        };

        let mut segment_words = Vec::new();
        let mut phrase = None;
        let mut needs_fallback = false;
        for mut morpheme in morphemes {
            let details = morpheme
                .details_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let part_of_speech = details.first().map(String::as_str).unwrap_or("");
            if part_of_speech == "記号" {
                finish_phrase(&mut phrase, &mut segment_words);
                continue;
            }
            let surface = morpheme.surface.as_ref().trim();
            if surface.is_empty() {
                continue;
            }
            let start_byte = morpheme.byte_start;
            let end_byte = morpheme.byte_end;
            let character_range = segment
                .text
                .char_indices()
                .enumerate()
                .filter_map(|(index, (byte, _))| {
                    (byte >= start_byte && byte < end_byte).then_some(index)
                })
                .collect::<Vec<_>>();
            let word_times = character_range
                .iter()
                .map(|&index| character_times.get(index).copied().flatten())
                .collect::<Option<Vec<_>>>();
            let Some(word_times) = word_times.filter(|times| !times.is_empty()) else {
                needs_fallback = true;
                break;
            };
            let start_ms = word_times
                .iter()
                .map(|(start, _)| *start)
                .min()
                .unwrap_or(segment.offsets.from)
                .max(segment.offsets.from);
            let end_ms = word_times
                .iter()
                .map(|(_, end)| *end)
                .max()
                .unwrap_or(segment.offsets.to)
                .min(segment.offsets.to);
            if end_ms <= start_ms {
                needs_fallback = true;
                break;
            }
            push_morpheme(
                TimedMorpheme {
                    surface: surface.to_owned(),
                    part_of_speech: part_of_speech.to_owned(),
                    subcategory: details.get(1).cloned().unwrap_or_default(),
                    conjugation_form: details.get(5).cloned().unwrap_or_default(),
                    byte_start: start_byte,
                    byte_end: end_byte,
                    start_ms,
                    end_ms,
                },
                &mut phrase,
                &mut segment_words,
            );
        }

        if needs_fallback {
            push_candidate_once(
                segment_candidate(
                    &segment.text,
                    segment.offsets.from,
                    segment.offsets.to,
                    language.as_deref(),
                    model,
                    version,
                ),
                &mut seen,
                &mut candidates,
            );
            continue;
        }
        finish_phrase(&mut phrase, &mut segment_words);
        for phrase in segment_words {
            push_candidate_once(
                AnalysisCandidate {
                    text: phrase.text,
                    original_text: Some(segment.text.clone()),
                    language: language.clone(),
                    start_ms: phrase.start_ms,
                    end_ms: phrase.end_ms,
                    model: model.into(),
                    model_version: version.into(),
                    confidence: None,
                    evidence: Some("Whisperの時刻と形態素解析でまとめた語句候補".into()),
                },
                &mut seen,
                &mut candidates,
            );
        }
    }
    Ok(candidates)
}

#[derive(Debug)]
struct TimedMorpheme {
    surface: String,
    part_of_speech: String,
    subcategory: String,
    conjugation_form: String,
    byte_start: usize,
    byte_end: usize,
    start_ms: u64,
    end_ms: u64,
}

#[derive(Debug)]
struct PhraseCandidate {
    text: String,
    start_ms: u64,
    end_ms: u64,
    byte_end: usize,
    last_part_of_speech: String,
    ends_with_particle: bool,
    incomplete_inflection: bool,
}

fn push_morpheme(
    morpheme: TimedMorpheme,
    phrase: &mut Option<PhraseCandidate>,
    phrases: &mut Vec<PhraseCandidate>,
) {
    if let Some(current) = phrase.as_ref()
        && current.byte_end != morpheme.byte_start
    {
        finish_phrase(phrase, phrases);
    }

    if morpheme.part_of_speech == "助詞" {
        if let Some(current) = phrase.as_mut() {
            current.text.push_str(&morpheme.surface);
            current.end_ms = current.end_ms.max(morpheme.end_ms);
            current.byte_end = morpheme.byte_end;
            current
                .last_part_of_speech
                .clone_from(&morpheme.part_of_speech);
            current.ends_with_particle = true;
        }
        return;
    }

    let is_suffix = morpheme.subcategory == "接尾";
    if morpheme.part_of_speech == "助動詞" || is_suffix {
        if let Some(current) = phrase.as_mut() {
            append_morpheme(current, &morpheme);
        }
        return;
    }

    let joins_compound = phrase.as_ref().is_some_and(|current| {
        !current.ends_with_particle
            && ((current.last_part_of_speech == "名詞" && morpheme.part_of_speech == "名詞")
                || current.last_part_of_speech == "接頭詞")
    });
    if joins_compound {
        append_morpheme(phrase.as_mut().unwrap(), &morpheme);
    } else {
        finish_phrase(phrase, phrases);
        *phrase = Some(PhraseCandidate {
            text: morpheme.surface.clone(),
            start_ms: morpheme.start_ms,
            end_ms: morpheme.end_ms,
            byte_end: morpheme.byte_end,
            last_part_of_speech: morpheme.part_of_speech.clone(),
            ends_with_particle: false,
            incomplete_inflection: is_incomplete_inflection(&morpheme),
        });
    }
}

fn append_morpheme(phrase: &mut PhraseCandidate, morpheme: &TimedMorpheme) {
    phrase.text.push_str(&morpheme.surface);
    phrase.start_ms = phrase.start_ms.min(morpheme.start_ms);
    phrase.end_ms = phrase.end_ms.max(morpheme.end_ms);
    phrase.byte_end = morpheme.byte_end;
    phrase
        .last_part_of_speech
        .clone_from(&morpheme.part_of_speech);
    phrase.ends_with_particle = false;
    phrase.incomplete_inflection = is_incomplete_inflection(morpheme);
}

fn is_incomplete_inflection(morpheme: &TimedMorpheme) -> bool {
    morpheme.part_of_speech == "動詞"
        && matches!(
            morpheme.conjugation_form.as_str(),
            "未然形" | "未然ウ接続" | "連用形" | "連用タ接続" | "連用ニ接続" | "体言接続特殊２"
        )
}

fn finish_phrase(phrase: &mut Option<PhraseCandidate>, phrases: &mut Vec<PhraseCandidate>) {
    if let Some(phrase) = phrase.take()
        && !phrase.incomplete_inflection
    {
        phrases.push(phrase);
    }
}

fn segment_candidate(
    text: &str,
    start_ms: u64,
    end_ms: u64,
    language: Option<&str>,
    model: &str,
    version: &str,
) -> AnalysisCandidate {
    AnalysisCandidate {
        text: text.to_owned(),
        original_text: Some(text.to_owned()),
        language: language.map(str::to_owned),
        start_ms,
        end_ms,
        model: model.into(),
        model_version: version.into(),
        confidence: None,
        evidence: Some("文字起こし区間（単語時刻を対応付けられませんでした）".into()),
    }
}

fn push_candidate_once(
    candidate: AnalysisCandidate,
    seen: &mut HashSet<String>,
    candidates: &mut Vec<AnalysisCandidate>,
) {
    if candidate.start_ms >= candidate.end_ms || seen.insert(candidate.text.clone()) {
        candidates.push(candidate);
    }
}

fn align_character_times(text: &str, tokens: &[WhisperToken]) -> Option<Vec<Option<(u64, u64)>>> {
    let source = text
        .char_indices()
        .enumerate()
        .filter(|(_, (_, character))| !character.is_whitespace())
        .map(|(index, (_, character))| (index, character))
        .collect::<Vec<_>>();
    let timed = tokens
        .iter()
        .filter(|token| !(token.text.starts_with("[_") && token.text.ends_with(']')))
        .flat_map(|token| {
            token
                .text
                .chars()
                .filter(|character| !character.is_whitespace())
                .map(move |character| {
                    let interval = (token.offsets.to > token.offsets.from)
                        .then_some((token.offsets.from, token.offsets.to));
                    (character, interval)
                })
        })
        .collect::<Vec<_>>();
    if source.is_empty() || timed.is_empty() {
        return None;
    }

    const UNMATCHED_COST: u32 = 1;
    const MISMATCH_COST: u32 = 3;
    let mut costs = vec![vec![0_u32; timed.len() + 1]; source.len() + 1];
    let mut paths = vec![vec![0_u8; timed.len() + 1]; source.len() + 1];
    for (index, row) in costs.iter_mut().enumerate().skip(1) {
        row[0] = index as u32 * UNMATCHED_COST;
        paths[index][0] = 1;
    }
    for index in 1..=timed.len() {
        costs[0][index] = index as u32 * UNMATCHED_COST;
        paths[0][index] = 2;
    }
    for source_index in 1..=source.len() {
        for timed_index in 1..=timed.len() {
            let source_character = source[source_index - 1].1;
            let timed_character = timed[timed_index - 1].0;
            let match_cost = if source_character == timed_character || timed_character == '\u{fffd}'
            {
                0
            } else {
                MISMATCH_COST
            };
            let options = [
                (costs[source_index - 1][timed_index - 1] + match_cost, 0_u8),
                (costs[source_index - 1][timed_index] + UNMATCHED_COST, 1_u8),
                (costs[source_index][timed_index - 1] + UNMATCHED_COST, 2_u8),
            ];
            let (cost, path) = options
                .into_iter()
                .min_by_key(|(cost, path)| (*cost, *path))
                .unwrap_or_default();
            costs[source_index][timed_index] = cost;
            paths[source_index][timed_index] = path;
        }
    }

    let mut result = vec![None; text.chars().count()];
    let mut source_index = source.len();
    let mut timed_index = timed.len();
    while source_index > 0 || timed_index > 0 {
        match paths[source_index][timed_index] {
            0 if source_index > 0 && timed_index > 0 => {
                let (original_index, source_character) = source[source_index - 1];
                let (timed_character, interval) = timed[timed_index - 1];
                if (source_character == timed_character || timed_character == '\u{fffd}')
                    && interval.is_some()
                {
                    result[original_index] = interval;
                }
                source_index -= 1;
                timed_index -= 1;
            }
            1 if source_index > 0 => source_index -= 1,
            2 if timed_index > 0 => timed_index -= 1,
            _ => return None,
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires local ffmpeg, whisper-cli, a model and CUTUP_TEST_INPUT"]
    fn splitter_transcribes_input_with_invalid_short_segment_utf8() {
        let input = PathBuf::from(std::env::var_os("CUTUP_TEST_INPUT").unwrap());
        let config = SttConfig {
            ffmpeg: "ffmpeg".into(),
            whisper: std::env::var_os("CUTUP_TEST_WHISPER").unwrap().into(),
            model: std::env::var_os("CUTUP_TEST_MODEL").unwrap().into(),
            language: SpeechLanguage::Auto,
        };
        let cancel = AtomicBool::new(false);
        let hash = fingerprint(&input, &cancel).unwrap();
        let result = transcribe_for_splitting(&config, &input, &hash, &cancel).unwrap();
        assert!(!result.is_empty());
        assert!(
            result
                .iter()
                .all(|segment| !segment.text.contains('\u{fffd}'))
        );
    }

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
            language: SpeechLanguage::Auto,
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
            language: SpeechLanguage::Auto,
        };
        assert!(matches!(
            transcribe(&config, &media, "old-fingerprint", &AtomicBool::new(false)),
            Err(SttError::MaterialChanged)
        ));
    }
    #[test]
    #[ignore = "requires local ffmpeg, whisper-cli and a model"]
    fn japanese_language_transcribes_demo_tekkaman() {
        let config = SttConfig {
            ffmpeg: std::env::var_os("CUTUP_TEST_FFMPEG").unwrap().into(),
            whisper: std::env::var_os("CUTUP_TEST_WHISPER").unwrap().into(),
            model: std::env::var_os("CUTUP_TEST_MODEL").unwrap().into(),
            language: SpeechLanguage::Japanese,
        };
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/materials/01-tekkaman.wav");
        let cancel = AtomicBool::new(false);
        let digest = fingerprint(&source, &cancel).unwrap();
        let candidates = transcribe(&config, &source, &digest, &cancel).unwrap();
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.text.contains("テッカマン"))
        );
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
            language: SpeechLanguage::Auto,
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

    #[test]
    fn splitting_returns_japanese_phrases_with_token_times() {
        let candidates = parse_word_transcript(
            r#"{"result":{"language":"ja"},"transcription":[{"text":"猫が走った。","offsets":{"from":0,"to":700},"tokens":[{"text":"猫","offsets":{"from":100,"to":200}},{"text":"が","offsets":{"from":200,"to":260}},{"text":"走","offsets":{"from":260,"to":400}},{"text":"っ","offsets":{"from":400,"to":450}},{"text":"た","offsets":{"from":450,"to":500}},{"text":"。","offsets":{"from":500,"to":550}}]}]}"#.as_bytes(),
            "ggml-tiny.bin",
            "sha256:test",
        )
        .unwrap();

        assert!(candidates.iter().any(|candidate| candidate.text == "猫が"));
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.text == "走った")
        );
        assert!(
            candidates.iter().all(|candidate| {
                candidate.start_ms < candidate.end_ms && candidate.text != "。"
            })
        );
    }

    #[test]
    fn splitting_keeps_the_first_valid_occurrence_of_each_surface() {
        let candidates = parse_word_transcript(
            r#"{"transcription":[{"text":"猫が猫が","offsets":{"from":0,"to":600},"tokens":[{"text":"猫","offsets":{"from":10,"to":100}},{"text":"が","offsets":{"from":100,"to":160}},{"text":"猫","offsets":{"from":300,"to":400}},{"text":"が","offsets":{"from":400,"to":460}}]}]}"#.as_bytes(),
            "ggml-tiny.bin",
            "sha256:test",
        )
        .unwrap();

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.text.as_str())
                .collect::<Vec<_>>(),
            ["猫が"]
        );
        assert_eq!((candidates[0].start_ms, candidates[0].end_ms), (10, 160));
    }

    #[test]
    fn splitting_groups_nouns_particles_and_verb_auxiliaries() {
        let candidates = parse_word_transcript(
            r#"{"transcription":[{"text":"青森の梨。今年も来た","offsets":{"from":0,"to":700},"tokens":[{"text":"青森","offsets":{"from":10,"to":180}},{"text":"の","offsets":{"from":180,"to":220}},{"text":"梨","offsets":{"from":220,"to":300}},{"text":"。","offsets":{"from":300,"to":320}},{"text":"今年","offsets":{"from":350,"to":430}},{"text":"も","offsets":{"from":430,"to":470}},{"text":"来","offsets":{"from":470,"to":530}},{"text":"た","offsets":{"from":530,"to":580}}]}]}"#.as_bytes(),
            "ggml-tiny.bin",
            "sha256:test",
        )
        .unwrap();

        let phrases = candidates
            .iter()
            .map(|candidate| candidate.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(phrases, ["青森の", "梨", "今年も", "来た"]);
        assert_eq!((candidates[0].start_ms, candidates[0].end_ms), (10, 220));
        assert_eq!((candidates[3].start_ms, candidates[3].end_ms), (470, 580));
    }

    #[test]
    fn splitting_joins_a_split_suffix_and_drops_standalone_fragments() {
        let joined = parse_word_transcript(
            r#"{"transcription":[{"text":"つがる","offsets":{"from":0,"to":100},"tokens":[{"text":"つ","offsets":{"from":10,"to":40}},{"text":"がる","offsets":{"from":40,"to":90}}]}]}"#.as_bytes(),
            "ggml-tiny.bin",
            "sha256:test",
        )
        .unwrap();
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].text, "つがる");
        assert_eq!((joined[0].start_ms, joined[0].end_ms), (10, 90));

        let fragments = parse_word_transcript(
            r#"{"transcription":[{"text":"としもきた","offsets":{"from":0,"to":300},"tokens":[{"text":"と","offsets":{"from":10,"to":30}},{"text":"し","offsets":{"from":30,"to":70}},{"text":"も","offsets":{"from":70,"to":110}},{"text":"き","offsets":{"from":110,"to":170}},{"text":"た","offsets":{"from":170,"to":210}}]}]}"#.as_bytes(),
            "ggml-tiny.bin",
            "sha256:test",
        )
        .unwrap();
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].text, "きた");
    }

    #[test]
    fn splitting_falls_back_to_the_segment_when_token_text_cannot_be_aligned() {
        let candidates = parse_word_transcript(
            r#"{"transcription":[{"text":"猫が","offsets":{"from":10,"to":300},"tokens":[{"text":"犬犬","offsets":{"from":10,"to":200}}]}]}"#.as_bytes(),
            "ggml-tiny.bin",
            "sha256:test",
        )
        .unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].text, "猫が");
        assert_eq!((candidates[0].start_ms, candidates[0].end_ms), (10, 300));
    }

    #[test]
    fn splitting_deduplicates_a_fallback_surface_against_later_word_candidates() {
        let candidates = parse_word_transcript(
            r#"{"transcription":[{"text":"猫","offsets":{"from":10,"to":100}},{"text":"猫","offsets":{"from":200,"to":300},"tokens":[{"text":"猫","offsets":{"from":200,"to":280}}]}]}"#.as_bytes(),
            "ggml-tiny.bin",
            "sha256:test",
        )
        .unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].text, "猫");
        assert_eq!((candidates[0].start_ms, candidates[0].end_ms), (10, 100));
    }

    #[test]
    fn splitting_recovers_a_malformed_token_byte_without_losing_text() {
        let mut json = r#"{"transcription":[{"text":"猫","offsets":{"from":0,"to":100},"tokens":[{"text":"x","offsets":{"from":10,"to":80}}]}]}"#.as_bytes().to_vec();
        let token_text = json
            .windows(3)
            .position(|window| window == b"\"x\"")
            .unwrap()
            + 1;
        json[token_text] = 0xff;

        let candidates = parse_word_transcript(&json, "ggml-tiny.bin", "sha256:test").unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].text, "猫");
        assert_eq!((candidates[0].start_ms, candidates[0].end_ms), (10, 80));
    }

    #[test]
    fn dtw_preset_is_derived_from_supported_model_filenames() {
        assert_eq!(dtw_model(Path::new("ggml-tiny.bin")), Some("tiny"));
        assert_eq!(
            dtw_model(Path::new("ggml-large-v3-turbo-q5_0.bin")),
            Some("large-v3-turbo")
        );
        assert_eq!(dtw_model(Path::new("custom-model.gguf")), None);
    }

    #[test]
    #[ignore = "requires CUTUP_TEST_DTW_JSON from a whisper.cpp -ojf -dtw run"]
    fn measures_issue_35_phrase_candidate_counts() {
        let json =
            fs::read(std::env::var_os("CUTUP_TEST_DTW_JSON").expect("set CUTUP_TEST_DTW_JSON"))
                .unwrap();
        let transcript: WordTranscript =
            serde_json::from_str(&String::from_utf8_lossy(&json)).unwrap();
        let segment_count = transcript.transcription.len();
        let config = serde_json::json!({ "dictionary": "embedded://ipadic", "mode": "normal" });
        let mut segmenter = Segmenter::from_config(&config).unwrap().into_worker();
        let mut morpheme_occurrences = 0;
        for segment in &transcript.transcription {
            for mut morpheme in segmenter.segment(&segment.text).unwrap() {
                if morpheme.details_iter().next() != Some("記号")
                    && !morpheme.surface.trim().is_empty()
                {
                    morpheme_occurrences += 1;
                }
            }
        }
        let candidates =
            parse_word_transcript(&json, "ggml-tiny.bin", "sha256:evaluation").unwrap();
        let unique_phrases = candidates
            .iter()
            .filter(|candidate| {
                candidate.evidence.as_deref() == Some("Whisperの時刻と形態素解析でまとめた語句候補")
            })
            .count();
        let segment_fallbacks = candidates.len() - unique_phrases;
        eprintln!(
            "segments={segment_count} morpheme_occurrences={morpheme_occurrences} unique_phrase_candidates={unique_phrases} segment_fallbacks={segment_fallbacks}"
        );
    }
}
