//! Ollama integration restricted to locally stored models and a loopback endpoint.
use crate::domain::valueobject::catalog::AnalysisCandidate;
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

pub fn summarize(
    model: &str,
    candidates: &[AnalysisCandidate],
) -> Result<AnalysisCandidate, OllamaError> {
    summarize_at("http://127.0.0.1:11434", model, candidates)
}

fn summarize_at(
    base: &str,
    model: &str,
    candidates: &[AnalysisCandidate],
) -> Result<AnalysisCandidate, OllamaError> {
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
    let transcript = candidates
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "以下は音声認識の未確認テキストです。内容と反応の候補を短く日本語で説明してください。不明なことは推測で断定しないでください。\n\n{transcript}"
    );
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
    Ok(AnalysisCandidate {
        original_text: Some(output.response.clone()),
        language: None,
        text: output.response,
        start_ms: candidates.first().map_or(0, |c| c.start_ms),
        end_ms: candidates.last().map_or(0, |c| c.end_ms),
        model: model.name,
        model_version: model.digest,
        confidence: None,
        evidence: Some("文字起こし候補の要約".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generates_a_separate_candidate_from_local_model_response() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for body in [
                r#"{"models":[{"name":"example:latest","digest":"abc","details":{"format":"gguf"}}]}"#,
                r#"{"response":"反応の候補","done":true}"#,
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 8192];
                loop {
                    let n = socket.read(&mut buffer).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let input = AnalysisCandidate {
            text: "こんにちは".into(),
            original_text: Some("こんにちは".into()),
            language: Some("ja".into()),
            start_ms: 100,
            end_ms: 800,
            model: "whisper".into(),
            model_version: "hash".into(),
            confidence: None,
            evidence: None,
        };
        let candidate = summarize_at(&format!("http://{address}"), "example", &[input]).unwrap();
        assert_eq!(candidate.text, "反応の候補");
        assert_eq!((candidate.start_ms, candidate.end_ms), (100, 800));
        assert_eq!(candidate.model_version, "abc");
        server.join().unwrap();
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
