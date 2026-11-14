//! Ollama integration restricted to locally stored models and a loopback endpoint.
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OllamaError {
    #[error("Ollama に接続できません。Ollama とローカルモデルを導入・起動してください")]
    Unavailable,
    #[error("指定されたローカルモデルが見つかりません。ollama list を確認してください")]
    ModelMissing,
    #[error("クラウドモデルは使えません。ローカルモデルを指定してください")]
    RemoteModel,
    #[error("Ollama の応答を読み取れません")]
    InvalidResponse,
    #[error("Ollama を起動できません。Ollama がインストールされているか確認してください")]
    StartFailed,
}

fn executable() -> Option<std::path::PathBuf> {
    let default = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .map(|p| p.join("Programs/Ollama/ollama.exe"))
        .filter(|p| p.is_file());
    default.or_else(|| {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| {
                    dir.join(if cfg!(windows) {
                        "ollama.exe"
                    } else {
                        "ollama"
                    })
                })
                .find(|path| path.is_file())
        })
    })
}

pub fn is_installed() -> bool {
    launch_executable().is_some()
}

fn launch_executable() -> Option<std::path::PathBuf> {
    let executable = executable()?;
    if cfg!(windows) {
        let app = executable.with_file_name("ollama app.exe");
        app.is_file().then_some(app)
    } else {
        Some(executable)
    }
}

pub enum StartOutcome {
    Started(Vec<String>),
    AlreadyRunning(Vec<String>),
}

pub fn start_and_list_models() -> Result<StartOutcome, OllamaError> {
    if let Ok(models) = local_models() {
        return Ok(StartOutcome::AlreadyRunning(models));
    }
    let executable = launch_executable().ok_or(OllamaError::StartFailed)?;
    let mut command = std::process::Command::new(executable);
    command
        .arg(if cfg!(windows) { "hidden" } else { "serve" })
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| OllamaError::StartFailed)?;
    for _ in 0..20 {
        if let Ok(models) = local_models() {
            return Ok(StartOutcome::Started(models));
        }
        if child
            .try_wait()
            .map_err(|_| OllamaError::StartFailed)?
            .is_some()
        {
            return local_models()
                .map(StartOutcome::Started)
                .map_err(|_| OllamaError::StartFailed);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Err(OllamaError::Unavailable)
}

#[derive(Deserialize)]
struct Models {
    models: Vec<Model>,
}
#[derive(Deserialize)]
struct Model {
    name: String,
    digest: String,
    #[serde(default)]
    remote_host: String,
    #[serde(default)]
    remote_model: String,
    details: ModelDetails,
}
#[derive(Deserialize)]
struct ModelDetails {
    format: String,
}

impl Model {
    fn is_local(&self) -> bool {
        self.remote_host.is_empty()
            && self.remote_model.is_empty()
            && !self.name.to_ascii_lowercase().contains("cloud")
            && !self.digest.is_empty()
            && self.details.format == "gguf"
    }
}
#[derive(Deserialize)]
struct Response {
    response: String,
    done: bool,
}

pub fn local_models() -> Result<Vec<String>, OllamaError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(2)))
        .build()
        .into();
    let mut response = agent
        .get("http://127.0.0.1:11434/api/tags")
        .call()
        .map_err(|_| OllamaError::Unavailable)?;
    let models: Models = response
        .body_mut()
        .read_json()
        .map_err(|_| OllamaError::InvalidResponse)?;
    Ok(models
        .models
        .into_iter()
        .filter(Model::is_local)
        .map(|m| m.name)
        .collect())
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct MetadataSuggestions {
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
}

impl MetadataSuggestions {
    pub(crate) fn normalized(mut self) -> Self {
        for values in [&mut self.keywords, &mut self.tags, &mut self.roles] {
            *values = values
                .iter()
                .flat_map(|value| value.split("」「"))
                .flat_map(|value| value.split([',', '，', '、']))
                .map(|value| value.trim().trim_matches(['「', '」']).trim())
                .filter(|value| !value.is_empty() && value.chars().count() <= 40)
                .take(5)
                .map(str::to_owned)
                .collect();
        }
        self
    }
}

pub fn suggest_metadata(model: &str, transcript: &str) -> Result<MetadataSuggestions, OllamaError> {
    let transcript: String = transcript.chars().take(4000).collect();
    let prompt = format!(
        "音声文字起こしから素材情報を抽出してください。JSON オブジェクトだけを返し、形式は {{\"keywords\":[\"...\"],\"tags\":[\"...\"],\"roles\":[\"...\"]}}。語句・分類・役割はそれぞれ1件ずつ配列の別の要素にし、1つの文字列に複数の項目や「」をまとめないでください。keywords は話された検索語、tags は内容の短い分類、roles は映像編集上の役割を必ず最適なものから選びます。短い一言でも役割を推定し、疑問や驚きの発話は「反応」、呼びかけや指示は「展開」、場面の導入は「導入」、間をつなぐ語は「つなぎ」とします。説明文は作らないでください。各配列は最大5件、各項目は40文字以内。\n\n文字起こし:\n{transcript}"
    );
    let response = generate_at("http://127.0.0.1:11434", model, &prompt)?;
    parse_metadata_response(&response)
}

fn parse_metadata_response(response: &str) -> Result<MetadataSuggestions, OllamaError> {
    let json = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let suggestions: MetadataSuggestions =
        serde_json::from_str(json).map_err(|_| OllamaError::InvalidResponse)?;
    Ok(suggestions.normalized())
}

fn generate_at(base: &str, model: &str, prompt: &str) -> Result<String, OllamaError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .into();
    let mut response = agent
        .get(format!("{base}/api/tags"))
        .call()
        .map_err(|_| OllamaError::Unavailable)?;
    let models: Models = response
        .body_mut()
        .read_json()
        .map_err(|_| OllamaError::InvalidResponse)?;
    let model = models
        .models
        .into_iter()
        .find(|m| {
            m.name == model || (model.find(':').is_none() && m.name == format!("{model}:latest"))
        })
        .ok_or(OllamaError::ModelMissing)?;
    if !model.is_local() {
        return Err(OllamaError::RemoteModel);
    }
    let mut response = agent
        .post(format!("{base}/api/generate"))
        .send_json(json!({"model":model.name,"prompt":prompt,"stream":false,"format":"json"}))
        .map_err(|_| OllamaError::Unavailable)?;
    let output: Response = response
        .body_mut()
        .read_json()
        .map_err(|_| OllamaError::InvalidResponse)?;
    if !output.done || output.response.trim().is_empty() {
        return Err(OllamaError::InvalidResponse);
    }
    Ok(output.response)
}

/// Produce a small set of search terms; catalog metadata stays local.
pub fn related_terms(model: &str, query: &str) -> Result<Vec<String>, OllamaError> {
    let query: String = query.chars().take(200).collect();
    let prompt = format!(
        "検索語「{query}」と同じ種類の具体的な別の対象名を、日本語で最大10個、各行に1語だけ出力してください。上位概念、形容詞、説明、番号、記号は不要です。"
    );
    let response = generate_terms_at("http://127.0.0.1:11434", model, &prompt)?;
    Ok(parse_related_terms(&response))
}

fn parse_related_terms(response: &str) -> Vec<String> {
    response
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && line.chars().count() <= 30)
        .take(10)
        .map(str::to_owned)
        .collect()
}

fn generate_terms_at(base: &str, model: &str, prompt: &str) -> Result<String, OllamaError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .into();
    let mut response = agent
        .get(format!("{base}/api/tags"))
        .call()
        .map_err(|_| OllamaError::Unavailable)?;
    let models: Models = response
        .body_mut()
        .read_json()
        .map_err(|_| OllamaError::InvalidResponse)?;
    let model = models
        .models
        .into_iter()
        .find(|m| {
            m.name == model || (model.find(':').is_none() && m.name == format!("{model}:latest"))
        })
        .ok_or(OllamaError::ModelMissing)?;
    if !model.is_local() {
        return Err(OllamaError::RemoteModel);
    }
    let mut response = agent
        .post(format!("{base}/api/generate"))
        .send_json(json!({"model":model.name,"prompt":prompt,"stream":false}))
        .map_err(|_| OllamaError::Unavailable)?;
    let output: Response = response
        .body_mut()
        .read_json()
        .map_err(|_| OllamaError::InvalidResponse)?;
    if !output.done || output.response.trim().is_empty() {
        return Err(OllamaError::InvalidResponse);
    }
    Ok(output.response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_response_accepts_json_and_bounds_each_field() {
        let mut keywords = vec!["語句".to_owned(); 7];
        keywords.push("x".repeat(41));
        let response = serde_json::json!({
            "keywords": keywords,
            "tags": ["反応", " "],
            "roles": ["導入"]
        })
        .to_string();
        let metadata = parse_metadata_response(&format!("```json\n{response}\n```")).unwrap();
        assert_eq!(metadata.keywords, vec!["語句"; 5]);
        assert_eq!(metadata.tags, vec!["反応"]);
        assert_eq!(metadata.roles, vec!["導入"]);
    }

    #[test]
    fn metadata_response_separates_tags_grouped_in_one_element() {
        let response = serde_json::json!({
            "keywords": ["何？"],
            "tags": ["質問」「確認", "「疑問」、問いかけ"],
            "roles": ["反応"]
        })
        .to_string();

        let metadata = parse_metadata_response(&response).unwrap();

        assert_eq!(metadata.tags, vec!["質問", "確認", "疑問", "問いかけ"]);
    }

    #[test]
    fn metadata_response_rejects_unstructured_text() {
        assert!(matches!(
            parse_metadata_response("推定された役割は反応です"),
            Err(OllamaError::InvalidResponse)
        ));
    }

    #[test]
    fn related_terms_are_bounded_and_ignore_blank_or_oversized_lines() {
        let input = format!(
            "\n{}\n{}",
            "長".repeat(31),
            (0..15)
                .map(|i| format!("語{i}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let terms = parse_related_terms(&input);
        assert_eq!(terms.len(), 10);
        assert_eq!(terms[0], "語0");
    }

    #[test]
    fn rejects_remote_models_even_when_their_name_looks_local() {
        let remote: Model = serde_json::from_str(r#"{"name":"model:latest","digest":"123","remote_host":"https://example.invalid","details":{"format":"gguf"}}"#).unwrap();
        assert!(!remote.is_local());
        let local: Model = serde_json::from_str(
            r#"{"name":"model:latest","digest":"123","details":{"format":"gguf"}}"#,
        )
        .unwrap();
        assert!(local.is_local());
    }
}
