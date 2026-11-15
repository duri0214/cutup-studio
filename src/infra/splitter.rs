use std::{
    path::Path,
    process::{Command, Stdio},
};

pub fn duration_seconds(input: &Path) -> Result<f64, String> {
    let mut command = Command::new("ffprobe");
    command
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(input)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .output()
        .map_err(|e| format!("ffprobe を起動できません: {e}"))?;
    if !output.status.success() {
        return Err("動画の長さを取得できません。".into());
    }
    let duration = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|_| "動画の長さを読み取れません。")?;
    if !duration.is_finite() || duration <= 0.0 {
        return Err("動画の長さが不正です。".into());
    }
    Ok(duration)
}

fn youtube_url(url: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(url).map_err(|_| "YouTube の動画 URL を入力してください。")?;
    if parsed.scheme() != "https"
        || !matches!(
            parsed.host_str(),
            Some("youtube.com" | "www.youtube.com" | "m.youtube.com" | "youtu.be")
        )
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
    {
        return Err("有効な YouTube の動画 URL を入力してください。".into());
    }
    let id = if parsed.host_str() == Some("youtu.be") {
        parsed.path().strip_prefix('/').map(str::to_owned)
    } else if parsed.path() == "/watch" {
        parsed
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.into_owned())
    } else {
        parsed.path().strip_prefix("/shorts/").map(str::to_owned)
    };
    if !id.as_deref().is_some_and(|id| {
        id.len() == 11
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    }) {
        return Err("YouTube の動画ページの URL を入力してください。".into());
    }
    Ok(parsed)
}

pub fn download_youtube(url: &str) -> Result<tempfile::TempDir, String> {
    let parsed = youtube_url(url)?;
    let dir = tempfile::tempdir().map_err(|e| format!("一時フォルダを作成できません: {e}"))?;
    let mut command = Command::new("yt-dlp");
    command
        .args([
            "--ignore-config",
            "--no-playlist",
            "--no-progress",
            "--no-part",
            "--socket-timeout",
            "15",
            "--retries",
            "2",
            "--js-runtimes",
            "node",
            "--js-runtimes",
            "deno",
            "--format",
            "bestaudio/best",
            "--extract-audio",
            "--audio-format",
            "wav",
            "-o",
        ])
        .arg(dir.path().join("input.%(ext)s"))
        .arg("--")
        .arg(parsed.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let status = command
        .status()
        .map_err(|e| format!("yt-dlp を起動できません。導入と PATH を確認してください: {e}"))?;
    if !status.success() || !dir.path().join("input.wav").is_file() {
        return Err("動画の音声を取得できません。URL、公開状態、yt-dlp、FFmpeg、JavaScript 実行環境を確認してください。".into());
    }
    Ok(dir)
}

pub fn cut(input: &Path, output: &Path, start: f64, end: f64) -> Result<(), String> {
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return Err("開始は0以上、終了は開始より後にしてください。".into());
    }
    if let (Ok(source), Ok(destination)) = (input.canonicalize(), output.canonicalize())
        && source == destination
    {
        return Err("元動画を保存先に指定できません。".into());
    }
    let parent = output.parent().ok_or("保存先が不正です。")?;
    let temp = render(input, parent, start, end)?;
    temp.persist(output)
        .map_err(|e| format!("保存できません: {}", e.error))?;
    Ok(())
}

fn render(
    input: &Path,
    parent: &Path,
    start: f64,
    end: f64,
) -> Result<tempfile::NamedTempFile, String> {
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return Err("開始は0以上、終了は開始より後にしてください。".into());
    }
    let temp = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile_in(parent)
        .map_err(|e| format!("一時ファイルを作成できません: {e}"))?;
    let mut command = Command::new("ffmpeg");
    command
        .args(["-nostdin", "-v", "error", "-xerror", "-y", "-i"])
        .arg(input)
        .args([
            "-ss",
            &start.to_string(),
            "-t",
            &(end - start).to_string(),
            "-vn",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
        ])
        .arg(temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let status = command
        .status()
        .map_err(|e| format!("FFmpegを起動できません: {e}"))?;
    if !status.success() {
        return Err("音声を切り出せません。動画とFFmpegを確認してください。".into());
    }
    if temp.as_file().metadata().map_err(|e| e.to_string())?.len() <= 78 {
        return Err("指定した範囲に音声がありません。時刻を確認してください。".into());
    }
    Ok(temp)
}

fn material_stem(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
        .take(48)
        .collect();
    let clean = clean.trim_matches(|c: char| c.is_whitespace() || c == '.');
    let reserved = clean
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    if clean.is_empty()
        || matches!(
            reserved.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
    {
        return "音声".into();
    }
    clean.into()
}

pub fn save_material(
    input: &Path,
    root: &Path,
    text: &str,
    start: f64,
    end: f64,
) -> Result<std::path::PathBuf, String> {
    let materials = root.join("materials");
    if !materials.is_dir() {
        return Err("基準フォルダの materials フォルダが見つかりません。".into());
    }
    let mut temp = render(input, &materials, start, end)?;
    let stem = material_stem(text);
    for index in 1..=10000 {
        let name = if index == 1 {
            format!("{stem}.wav")
        } else {
            format!("{stem}-{index}.wav")
        };
        let output = materials.join(name);
        match temp.persist_noclobber(&output) {
            Ok(_) => return Ok(output),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temp = error.file
            }
            Err(error) => return Err(format!("保存できません: {}", error.error)),
        }
    }
    Err("同じ名前の素材が多すぎます。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_names_are_safe_and_readable() {
        assert_eq!(material_stem("  女性も？/ついふみ…  "), "女性も？ついふみ…");
        assert_eq!(material_stem("../"), "音声");
        assert_eq!(material_stem("CON.txt"), "音声");
        assert_eq!(material_stem("CON .txt"), "音声");
        assert_eq!(material_stem(&"あ".repeat(60)).chars().count(), 48);
    }
    #[test]
    fn accepts_only_youtube_video_urls() {
        assert!(youtube_url("https://www.youtube.com/watch?v=BaW_jenozKc").is_ok());
        assert!(youtube_url("https://youtu.be/BaW_jenozKc").is_ok());
        assert!(youtube_url("https://www.youtube.com/shorts/BaW_jenozKc").is_ok());
        assert_eq!(
            youtube_url("https://example.com/video").unwrap_err(),
            "有効な YouTube の動画 URL を入力してください。"
        );
        assert!(youtube_url("https://youtube.com.evil.test/watch?v=BaW_jenozKc").is_err());
        assert!(youtube_url("https://www.youtube.com/playlist?list=abc").is_err());
        assert!(youtube_url("https://www.youtube.com/watch?v=x").is_err());
    }

    #[test]
    #[ignore = "requires network, yt-dlp, FFmpeg, whisper-cli and a model"]
    fn downloads_youtube_audio_to_temporary_directory() {
        let url = std::env::var("CUTUP_TEST_YOUTUBE_URL").unwrap();
        let dir = download_youtube(&url).unwrap();
        let input = dir.path().join("input.wav");
        assert!(input.metadata().unwrap().len() > 78);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let config = crate::infra::local_ai::local_stt::SttConfig {
            ffmpeg: "ffmpeg".into(),
            whisper: std::env::var_os("CUTUP_TEST_WHISPER").unwrap().into(),
            model: std::env::var_os("CUTUP_TEST_MODEL").unwrap().into(),
            language: crate::infra::local_ai::local_stt::SpeechLanguage::Auto,
        };
        let hash = crate::infra::local_ai::local_stt::fingerprint(&input, &cancel).unwrap();
        let segments =
            crate::infra::local_ai::local_stt::transcribe(&config, &input, &hash, &cancel).unwrap();
        assert!(!segments.is_empty());
        let output = dir.path().join("clip.wav");
        cut(&input, &output, 0.0, 1.0).unwrap();
        assert!(output.is_file());
        let path = dir.path().to_owned();
        drop(dir);
        assert!(!path.exists());
    }
    #[test]
    fn invalid_range_and_source_destination_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("video.wav");
        std::fs::write(&input, b"original").unwrap();
        assert!(cut(&input, &dir.path().join("out.wav"), 2.0, 1.0).is_err());
        assert!(cut(&input, &input, 0.0, 1.0).is_err());
        assert_eq!(std::fs::read(input).unwrap(), b"original");
        assert!(
            save_material(Path::new("unused"), dir.path(), "台詞", 0.0, 1.0)
                .unwrap_err()
                .contains("materials")
        );
    }

    #[test]
    #[ignore = "requires FFmpeg on PATH"]
    fn ffmpeg_cuts_audio_and_rejects_range_past_end() {
        let dir = tempfile::tempdir().unwrap();
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/materials/01-tekkaman.wav");
        let duration = duration_seconds(&input).unwrap();
        assert!(duration > 0.2);
        let output = dir.path().join("cut.wav");
        cut(&input, &output, 0.0, 0.2).unwrap();
        assert!(std::fs::metadata(&output).unwrap().len() > 78);
        let missing = dir.path().join("empty.wav");
        assert!(cut(&input, &missing, 9999.0, 10000.0).is_err());
        assert!(!missing.exists());
        std::fs::create_dir(dir.path().join("materials")).unwrap();
        let existing = dir.path().join("materials").join("テッカマン.wav");
        std::fs::write(&existing, b"original").unwrap();
        let saved = save_material(&input, dir.path(), "テッカマン", 0.0, 0.2).unwrap();
        assert!(saved.ends_with("テッカマン-2.wav"));
        assert_eq!(std::fs::read(existing).unwrap(), b"original");
    }
}
