use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn cli_reuses_validated_compilation_cache() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("linxts-cli-cache-{}-{nonce}", std::process::id()));
    let source_path = root.join("cached.lx");
    let cache_path = root.join("cache");
    std::fs::create_dir_all(&root).expect("create fixture directory");
    std::fs::write(&source_path, "PRINT 7\n").expect("write fixture");

    let first = run(&source_path, &cache_path);
    assert!(first.status.success());
    assert_eq!(String::from_utf8_lossy(&first.stdout), "7\n");
    assert!(String::from_utf8_lossy(&first.stderr).contains("Cache: miss"));

    let second = run(&source_path, &cache_path);
    assert!(second.status.success());
    assert_eq!(String::from_utf8_lossy(&second.stdout), "7\n");
    assert!(String::from_utf8_lossy(&second.stderr).contains("Cache: hit"));

    let _ = std::fs::remove_dir_all(root);
}

fn run(source: &std::path::Path, cache: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_linxts"))
        .arg("run")
        .arg(source)
        .arg("--verbose")
        .arg("--no-ai")
        .env("LINXTS_CACHE_DIR", cache)
        .output()
        .expect("run linxts")
}
