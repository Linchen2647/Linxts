use crate::protocol::{CandidateEnvelope, candidate_json_schema};
use crate::provider::{
    AiProvider, AiRequest, CancellationToken, InferenceBudget, ProviderError, ProviderHealth,
};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpEndpoint {
    host: String,
    port: u16,
    base_path: String,
    loopback: bool,
}

impl HttpEndpoint {
    pub fn parse(value: &str) -> Result<Self, ProviderError> {
        let Some(authority_and_path) = value.strip_prefix("http://") else {
            return Err(ProviderError::Unavailable(
                "llama.cpp endpoint must use http://; TLS endpoints are not supported by this local client"
                    .to_owned(),
            ));
        };
        if authority_and_path.contains(['\r', '\n', '@', '#', '?']) {
            return Err(ProviderError::Unavailable(
                "endpoint contains credentials or unsupported URL characters".to_owned(),
            ));
        }
        let (authority, path) = authority_and_path
            .split_once('/')
            .map_or((authority_and_path, ""), |(authority, path)| {
                (authority, path)
            });
        if authority.is_empty() {
            return Err(ProviderError::Unavailable(
                "endpoint host is empty".to_owned(),
            ));
        }
        let (host, port) = parse_authority(authority)?;
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        let base_path = if path.is_empty() {
            String::new()
        } else {
            format!("/{}", path.trim_end_matches('/'))
        };
        Ok(Self {
            host,
            port,
            base_path,
            loopback,
        })
    }

    #[must_use]
    pub fn is_loopback(&self) -> bool {
        self.loopback
    }

    fn path(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.base_path)
    }

    fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlamaCppProvider {
    id: String,
    model_id: String,
    endpoint: HttpEndpoint,
}

impl LlamaCppProvider {
    pub fn new(
        id: impl Into<String>,
        model_id: impl Into<String>,
        endpoint: &str,
    ) -> Result<Self, ProviderError> {
        let endpoint = HttpEndpoint::parse(endpoint)?;
        if !endpoint.is_loopback() {
            return Err(ProviderError::Unavailable(
                "non-loopback llama.cpp endpoint rejected; use new_allow_remote only after reviewing source-upload privacy"
                    .to_owned(),
            ));
        }
        Ok(Self {
            id: id.into(),
            model_id: model_id.into(),
            endpoint,
        })
    }

    pub fn new_allow_remote(
        id: impl Into<String>,
        model_id: impl Into<String>,
        endpoint: &str,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            id: id.into(),
            model_id: model_id.into(),
            endpoint: HttpEndpoint::parse(endpoint)?,
        })
    }

    fn completion_body(
        &self,
        request: &AiRequest,
        budget: &InferenceBudget,
    ) -> Result<Vec<u8>, ProviderError> {
        let input = serde_json::json!({
            "source": request.source,
            "source_hash": request.source_hash,
            "language_hint": request.language_hint,
            "parser_diagnostics": request.diagnostics,
        });
        let prompt = format!(
            "You are the Linxts compiler fallback. Return exactly one JSON object matching the supplied schema. Convert the source intent into the current Linxts IR schema. Preserve source_hash exactly. Do not emit markdown, prose, external effects, filesystem operations, process execution, networking, imports, FFI, or unresolved error nodes. If intent is unsafe or cannot be represented, do not fabricate capabilities.\nINPUT JSON:\n{}",
            serde_json::to_string(&input)
                .map_err(|error| ProviderError::InvalidJson(error.to_string()))?
        );
        let context_byte_budget = (budget.context_tokens as usize).saturating_mul(4);
        if prompt.len() > context_byte_budget {
            return Err(ProviderError::PromptTooLarge {
                actual: prompt.len(),
                limit: context_byte_budget,
            });
        }
        serde_json::to_vec(&serde_json::json!({
            "prompt": prompt,
            "n_predict": budget.output_tokens,
            "temperature": 0.0,
            "cache_prompt": true,
            "json_schema": candidate_json_schema(),
        }))
        .map_err(|error| ProviderError::InvalidJson(error.to_string()))
    }
}

impl AiProvider for LlamaCppProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn health(&self, timeout: Duration) -> Result<ProviderHealth, ProviderError> {
        let response = request(
            &self.endpoint,
            "GET",
            &self.endpoint.path("/health"),
            &[],
            timeout,
            64 * 1024,
        )?;
        if !(200..300).contains(&response.status) {
            return Ok(ProviderHealth {
                available: false,
                detail: format!("HTTP {}", response.status),
            });
        }
        let detail = serde_json::from_slice::<Value>(&response.body)
            .ok()
            .and_then(|value| {
                value
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "HTTP health check succeeded".to_owned());
        Ok(ProviderHealth {
            available: true,
            detail,
        })
    }

    fn generate(
        &self,
        request_data: &AiRequest,
        budget: &InferenceBudget,
        cancellation: &CancellationToken,
    ) -> Result<CandidateEnvelope, ProviderError> {
        request_data.validate(budget)?;
        cancellation.check()?;
        let body = self.completion_body(request_data, budget)?;
        let response = request(
            &self.endpoint,
            "POST",
            &self.endpoint.path("/completion"),
            &body,
            budget.timeout,
            budget.max_response_bytes,
        )?;
        cancellation.check()?;
        if !(200..300).contains(&response.status) {
            return Err(ProviderError::Http {
                status: response.status,
                message: bounded_text(&response.body, 512),
            });
        }
        let outer: Value = serde_json::from_slice(&response.body)
            .map_err(|error| ProviderError::InvalidJson(error.to_string()))?;
        let content = outer
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidJson(
                    "llama.cpp response did not contain string field `content`".to_owned(),
                )
            })?;
        let mut candidate: CandidateEnvelope = serde_json::from_str(content)
            .map_err(|error| ProviderError::InvalidJson(error.to_string()))?;
        candidate.set_provider("llama_cpp", &self.model_id);
        candidate.validate(&request_data.source_hash)?;
        Ok(candidate)
    }
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn request(
    endpoint: &HttpEndpoint,
    method: &str,
    path: &str,
    body: &[u8],
    timeout: Duration,
    max_response_bytes: usize,
) -> Result<HttpResponse, ProviderError> {
    let address = resolve(endpoint, timeout)?;
    let mut stream = TcpStream::connect_timeout(&address, timeout).map_err(map_io_error)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(map_io_error)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(map_io_error)?;
    let headers = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.authority(),
        body.len()
    );
    stream.write_all(headers.as_bytes()).map_err(map_io_error)?;
    stream.write_all(body).map_err(map_io_error)?;
    stream.flush().map_err(map_io_error)?;

    let hard_limit = max_response_bytes.saturating_add(64 * 1024);
    let mut raw = Vec::new();
    stream
        .take((hard_limit + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(map_io_error)?;
    if raw.len() > hard_limit {
        return Err(ProviderError::ResponseTooLarge {
            limit: max_response_bytes,
        });
    }
    parse_response(&raw, max_response_bytes)
}

fn resolve(endpoint: &HttpEndpoint, _timeout: Duration) -> Result<SocketAddr, ProviderError> {
    endpoint
        .authority()
        .to_socket_addrs()
        .map_err(map_io_error)?
        .next()
        .ok_or_else(|| ProviderError::Unavailable("endpoint did not resolve".to_owned()))
}

fn parse_response(raw: &[u8], max_response_bytes: usize) -> Result<HttpResponse, ProviderError> {
    let Some(split) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Err(ProviderError::Transport(
            "malformed HTTP response headers".to_owned(),
        ));
    };
    let header_bytes = &raw[..split];
    let payload = &raw[split + 4..];
    let headers = std::str::from_utf8(header_bytes)
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| ProviderError::Transport("missing HTTP status".to_owned()))?;
    let chunked = lines.any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("transfer-encoding")
                && value.trim().eq_ignore_ascii_case("chunked")
        })
    });
    let body = if chunked {
        decode_chunked(payload, max_response_bytes)?
    } else {
        if payload.len() > max_response_bytes {
            return Err(ProviderError::ResponseTooLarge {
                limit: max_response_bytes,
            });
        }
        payload.to_vec()
    };
    Ok(HttpResponse { status, body })
}

fn decode_chunked(payload: &[u8], max: usize) -> Result<Vec<u8>, ProviderError> {
    let mut cursor = 0;
    let mut decoded = Vec::new();
    loop {
        let Some(line_end) = payload[cursor..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .map(|relative| cursor + relative)
        else {
            return Err(ProviderError::Transport(
                "malformed chunked response".to_owned(),
            ));
        };
        let size_text = std::str::from_utf8(&payload[cursor..line_end])
            .map_err(|error| ProviderError::Transport(error.to_string()))?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or_default(), 16)
            .map_err(|error| ProviderError::Transport(error.to_string()))?;
        cursor = line_end + 2;
        if size == 0 {
            break;
        }
        let end = cursor.saturating_add(size);
        if end + 2 > payload.len() || &payload[end..end + 2] != b"\r\n" {
            return Err(ProviderError::Transport(
                "truncated chunked response".to_owned(),
            ));
        }
        if decoded.len().saturating_add(size) > max {
            return Err(ProviderError::ResponseTooLarge { limit: max });
        }
        decoded.extend_from_slice(&payload[cursor..end]);
        cursor = end + 2;
    }
    Ok(decoded)
}

fn parse_authority(authority: &str) -> Result<(String, u16), ProviderError> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, port_text)) = rest.split_once("]:") else {
            return Err(ProviderError::Unavailable(
                "invalid bracketed endpoint authority".to_owned(),
            ));
        };
        let port = port_text
            .parse()
            .map_err(|_| ProviderError::Unavailable("invalid endpoint port".to_owned()))?;
        return Ok((host.to_owned(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port_text)) if !host.contains(':') => {
            let port = port_text
                .parse()
                .map_err(|_| ProviderError::Unavailable("invalid endpoint port".to_owned()))?;
            Ok((host.to_owned(), port))
        }
        Some(_) => Err(ProviderError::Unavailable(
            "IPv6 endpoints must use brackets".to_owned(),
        )),
        None => Ok((authority.to_owned(), 8080)),
    }
}

fn map_io_error(error: std::io::Error) -> ProviderError {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        ProviderError::Timeout
    } else {
        ProviderError::Transport(error.to_string())
    }
}

fn bounded_text(bytes: &[u8], limit: usize) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(limit)]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{AI_SCHEMA_VERSION, CandidateLanguage, CandidateStatus};
    use linxts_ir::{Expression, Module, Statement, StatementKind};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn endpoint_defaults_to_local_port() {
        let endpoint = HttpEndpoint::parse("http://127.0.0.1").expect("endpoint");
        assert_eq!(endpoint.port, 8080);
        assert!(endpoint.is_loopback());
    }

    #[test]
    fn rejects_https_and_credentials() {
        assert!(HttpEndpoint::parse("https://localhost:8080").is_err());
        assert!(HttpEndpoint::parse("http://user@localhost:8080").is_err());
    }

    #[test]
    fn decodes_chunked_body() {
        let decoded =
            decode_chunked(b"4\r\ntest\r\n3\r\n123\r\n0\r\n\r\n", 20).expect("valid chunked body");
        assert_eq!(decoded, b"test123");
    }

    #[test]
    fn accepts_valid_structured_completion() {
        let candidate = valid_candidate();
        let outer = serde_json::json!({
            "content": serde_json::to_string(&candidate).expect("candidate JSON")
        });
        let (url, server) = serve_once(serde_json::to_vec(&outer).expect("outer JSON"), None);
        let provider = LlamaCppProvider::new("3b", "fixture-3b", &url).expect("provider");
        let result = provider
            .generate(
                &ai_request(),
                &InferenceBudget {
                    timeout: Duration::from_secs(1),
                    ..InferenceBudget::default()
                },
                &CancellationToken::default(),
            )
            .expect("valid candidate");
        server.join().expect("server thread");
        assert_eq!(
            result.provider.expect("provider metadata").model_id,
            "fixture-3b"
        );
    }

    #[test]
    fn rejects_invalid_candidate_json() {
        let outer = serde_json::json!({"content": "not JSON"});
        let (url, server) = serve_once(serde_json::to_vec(&outer).expect("outer JSON"), None);
        let provider = LlamaCppProvider::new("3b", "fixture-3b", &url).expect("provider");
        let error = provider
            .generate(
                &ai_request(),
                &InferenceBudget::default(),
                &CancellationToken::default(),
            )
            .expect_err("invalid JSON");
        server.join().expect("server thread");
        assert!(matches!(error, ProviderError::InvalidJson(_)));
    }

    #[test]
    fn enforces_response_timeout() {
        let (url, server) = serve_once(b"{}".to_vec(), Some(Duration::from_millis(100)));
        let provider = LlamaCppProvider::new("3b", "fixture-3b", &url).expect("provider");
        let error = provider
            .generate(
                &ai_request(),
                &InferenceBudget {
                    timeout: Duration::from_millis(20),
                    ..InferenceBudget::default()
                },
                &CancellationToken::default(),
            )
            .expect_err("timeout");
        server.join().expect("server thread");
        assert!(matches!(error, ProviderError::Timeout));
    }

    fn ai_request() -> AiRequest {
        AiRequest {
            source: "unresolved intent".to_owned(),
            source_hash: "hash".to_owned(),
            language_hint: Some("natural".to_owned()),
            diagnostics: vec!["LXT-120".to_owned()],
        }
    }

    fn valid_candidate() -> CandidateEnvelope {
        let span = Default::default();
        let mut ir = Module::new("hash");
        ir.body.statements.push(Statement::new(
            StatementKind::Print(Expression::integer(1, span)),
            span,
        ));
        CandidateEnvelope {
            schema_version: AI_SCHEMA_VERSION.to_owned(),
            status: CandidateStatus::Candidate,
            language: CandidateLanguage::Natural,
            interpretation: "Print one".to_owned(),
            ir,
            assumptions: Vec::new(),
            warnings: Vec::new(),
            provider: None,
        }
    }

    fn serve_once(body: Vec<u8>, delay: Option<Duration>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let address = listener.local_addr().expect("fixture address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept fixture request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).expect("read fixture request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            if let Some(delay) = delay {
                thread::sleep(delay);
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&body);
        });
        (format!("http://{address}"), handle)
    }
}
