use linxts_ai::{AI_SCHEMA_VERSION, CandidateEnvelope, CandidateLanguage, CandidateStatus};
use linxts_ir::{Expression, Module, Statement, StatementKind};
use linxts_source::{SourceFile, SourceId};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn cli_uses_validated_local_ai_fallback() {
    let source_text = "^自然語言\n請執行一個尚未被規則涵蓋的純輸出意圖\n";
    let source = SourceFile::from_text(SourceId(0), "ai-fallback.lx", source_text);
    let candidate = candidate(source.snapshot_hash());
    let outer = serde_json::json!({
        "content": serde_json::to_string(&candidate).expect("candidate JSON")
    });
    let (endpoint, server) = serve_llama(serde_json::to_vec(&outer).expect("response JSON"));

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "linxts-ai-fallback-{}-{unique}.lx",
        std::process::id()
    ));
    std::fs::write(&path, source_text).expect("write source fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_linxts"))
        .arg("run")
        .arg(&path)
        .arg("--verbose")
        .arg("--no-cache")
        .env("LINXTS_LLAMA_3B_URL", endpoint)
        .env("LINXTS_LLAMA_3B_MODEL", "mock-3b")
        .env_remove("LINXTS_LLAMA_7B_URL")
        .env_remove("LINXTS_LLAMA_LARGE_URL")
        .output()
        .expect("run linxts");

    let _ = std::fs::remove_file(path);
    server.join().expect("mock server");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("AI invoked: true"), "stderr: {stderr}");
    assert!(stderr.contains("tree-walking VM"), "stderr: {stderr}");
}

fn candidate(source_hash: &str) -> CandidateEnvelope {
    let span = Default::default();
    let mut ir = Module::new(source_hash);
    ir.body.statements.push(Statement::new(
        StatementKind::Print(Expression::integer(42, span)),
        span,
    ));
    CandidateEnvelope {
        schema_version: AI_SCHEMA_VERSION.to_owned(),
        status: CandidateStatus::Candidate,
        language: CandidateLanguage::Natural,
        interpretation: "Print the integer 42 without external effects".to_owned(),
        ir,
        assumptions: Vec::new(),
        warnings: Vec::new(),
        provider: None,
    }
}

fn serve_llama(completion_body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock llama server");
    let address = listener.local_addr().expect("mock address");
    let handle = thread::spawn(move || {
        for request_number in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept request");
            let request = read_request(&mut stream);
            let body = if request_number == 0 {
                assert!(request.starts_with(b"GET /health "));
                br#"{"status":"ok"}"#.to_vec()
            } else {
                assert!(request.starts_with(b"POST /completion "));
                completion_body.clone()
            };
            write_response(&mut stream, &body);
        }
    });
    (format!("http://{address}"), handle)
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
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
    request
}

fn write_response(stream: &mut TcpStream, body: &[u8]) {
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes()).expect("write headers");
    stream.write_all(body).expect("write body");
}
