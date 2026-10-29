#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn builds_and_runs_native_elf() {
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping native integration test: cc unavailable");
            return;
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("linxts-native-test-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create fixture directory");
        let source = root.join("hello.lx");
        let executable = root.join("hello");
        std::fs::write(&source, "x = 41 + 1\nprint(f\"value={x}\")\n").expect("write fixture");

        let build = Command::new(env!("CARGO_BIN_EXE_linxts"))
            .arg("build")
            .arg(&source)
            .arg("--no-ai")
            .arg("--no-cache")
            .arg("--output")
            .arg(&executable)
            .output()
            .expect("run native build");
        assert!(
            build.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let bytes = std::fs::read(&executable).expect("read executable");
        assert_eq!(&bytes[..4], b"\x7fELF");

        let run = Command::new(&executable)
            .output()
            .expect("run native executable");
        assert!(run.status.success());
        assert_eq!(String::from_utf8_lossy(&run.stdout), "value=42\n");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn builds_and_runs_simplified_chinese_mixed_sample() {
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping native integration test: cc unavailable");
            return;
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "linxts-native-user-mixed-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create fixture directory");
        let source = root.join("mixed-user.lx");
        let executable = root.join("mixed-user");
        std::fs::write(
            &source,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/mixed-simplified.lx"
            )),
        )
        .expect("write fixture");

        let build = Command::new(env!("CARGO_BIN_EXE_linxts"))
            .arg("build")
            .arg(&source)
            .arg("--no-ai")
            .arg("--no-cache")
            .arg("--output")
            .arg(&executable)
            .output()
            .expect("run native build");
        assert!(
            build.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let bytes = std::fs::read(&executable).expect("read executable");
        assert_eq!(&bytes[..4], b"\x7fELF");

        let run = Command::new(&executable)
            .output()
            .expect("run native executable");
        assert!(run.status.success());
        assert_eq!(String::from_utf8_lossy(&run.stdout), "9\n720\n5\n");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn builds_cross_language_functions_loops_and_formats() {
        if Command::new("cc").arg("--version").output().is_err() {
            eprintln!("skipping native integration test: cc unavailable");
            return;
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "linxts-native-mixed-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create fixture directory");
        let source = root.join("mixed.lx");
        let executable = root.join("mixed");
        std::fs::write(
            &source,
            "^python\ndef add_one(value):\n    return value + 1\nseed = 4\n^javascript\nlet shared = add_one(seed);\nconsole.log(`js=${shared}`);\n^rust\nlet count = 0;\nwhile count < 3 {\n    count = count + 1;\n}\nprintln!(\"rs={}:{}\", shared, count);\n^c\nprintf(\"c=%d\", shared + count);\n",
        )
        .expect("write fixture");

        let build = Command::new(env!("CARGO_BIN_EXE_linxts"))
            .arg("build")
            .arg(&source)
            .arg("--no-ai")
            .arg("--no-cache")
            .arg("--output")
            .arg(&executable)
            .output()
            .expect("run native build");
        assert!(
            build.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let bytes = std::fs::read(&executable).expect("read executable");
        assert_eq!(&bytes[..4], b"\x7fELF");

        let run = Command::new(&executable)
            .output()
            .expect("run native executable");
        assert!(run.status.success());
        assert_eq!(String::from_utf8_lossy(&run.stdout), "js=5\nrs=5:3\nc=8\n");
        let _ = std::fs::remove_dir_all(root);
    }
}
