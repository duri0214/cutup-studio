use std::{
    path::Path,
    process::{Command, Stdio},
};

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
    temp.persist(output)
        .map_err(|e| format!("保存できません: {}", e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_range_and_source_destination_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("video.wav");
        std::fs::write(&input, b"original").unwrap();
        assert!(cut(&input, &dir.path().join("out.wav"), 2.0, 1.0).is_err());
        assert!(cut(&input, &input, 0.0, 1.0).is_err());
        assert_eq!(std::fs::read(input).unwrap(), b"original");
    }

    #[test]
    #[ignore = "requires FFmpeg on PATH"]
    fn ffmpeg_cuts_audio_and_rejects_range_past_end() {
        let dir = tempfile::tempdir().unwrap();
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/materials/01-tekkaman.wav");
        let output = dir.path().join("cut.wav");
        cut(&input, &output, 0.0, 0.2).unwrap();
        assert!(std::fs::metadata(&output).unwrap().len() > 78);
        let missing = dir.path().join("empty.wav");
        assert!(cut(&input, &missing, 9999.0, 10000.0).is_err());
        assert!(!missing.exists());
    }
}
