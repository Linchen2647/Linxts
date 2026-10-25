mod ai_support;

use clap::{Args, Parser, Subcommand};
use linxts_compiler::{
    CacheKeyInput, Compilation, CompilationCache, CompileOptions, Compiler, Diagnostic, Severity,
};
use linxts_runtime::Vm;
use linxts_source::{SourceFile, SourceId};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "linxts", version, about = "Linxts Adaptive Compilation System")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Compile a .lx file to a versioned VM artifact (.lxb).
    Build {
        source: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        flags: CompileFlags,
    },
    /// Compile and execute a .lx file with the Linxts VM.
    Run {
        source: PathBuf,
        #[command(flatten)]
        flags: CompileFlags,
    },
    /// Parse and validate a .lx file without running it.
    Check {
        source: PathBuf,
        #[command(flatten)]
        flags: CompileFlags,
    },
    /// Show detection, repair, and IR details.
    Explain {
        source: PathBuf,
        #[command(flatten)]
        flags: CompileFlags,
    },
    /// Inspect configured model backends.
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
    /// Show compiler configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Inspect local compiler and inference prerequisites.
    Doctor,
}

#[derive(Debug, Subcommand)]
enum ModelsCommand {
    List,
    Test,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Show,
}

#[derive(Debug, Clone, Args)]
struct CompileFlags {
    /// Enable deterministic aggressive repair (default).
    #[arg(long, default_value_t = true, conflicts_with = "strict")]
    aggressive: bool,
    /// Reject repairs above low semantic risk.
    #[arg(long)]
    strict: bool,
    /// Disable all AI fallback.
    #[arg(long)]
    no_ai: bool,
    /// Prohibit online providers.
    #[arg(long)]
    offline: bool,
    /// Disable reading and writing the validated compilation cache.
    #[arg(long)]
    no_cache: bool,
    /// Print the validated Linxts IR.
    #[arg(long)]
    show_ir: bool,
    /// Print pipeline details.
    #[arg(long)]
    verbose: bool,
    /// Add an informal line after formal diagnostics.
    #[arg(long)]
    fun_errors: bool,
}

impl CompileFlags {
    fn options(&self) -> CompileOptions {
        CompileOptions {
            aggressive: self.aggressive || !self.strict,
            strict: self.strict,
            no_ai: self.no_ai,
            offline: self.offline,
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

fn run(cli: Cli) -> Result<(), u8> {
    match cli.command {
        Command::Build {
            source,
            output,
            flags,
        } => {
            let (source_file, compilation) = compile_path(&source, &flags)?;
            emit_optional_details(&source_file, &compilation, &flags);
            if compilation.has_errors() {
                emit_diagnostics(&source_file, &compilation.diagnostics, flags.fun_errors);
                return Err(1);
            }
            let output = output.unwrap_or_else(|| source.with_extension("lxb"));
            let json = serde_json::to_vec_pretty(&compilation.module).map_err(|error| {
                eprintln!("[ERROR LXT-BLD-001] Failed to encode VM artifact: {error}");
                2
            })?;
            std::fs::write(&output, json).map_err(|error| {
                eprintln!(
                    "[ERROR LXT-BLD-002] Failed to write {}: {error}",
                    output.display()
                );
                2
            })?;
            eprintln!(
                "[OK] Wrote Linxts VM artifact: {} (not a native executable)",
                output.display()
            );
            Ok(())
        }
        Command::Run { source, flags } => {
            let (source_file, compilation) = compile_path(&source, &flags)?;
            emit_optional_details(&source_file, &compilation, &flags);
            if compilation.has_errors() {
                emit_diagnostics(&source_file, &compilation.diagnostics, flags.fun_errors);
                return Err(1);
            }
            eprintln!("[Linxts] Backend: tree-walking VM (native ELF is not implemented yet)");
            let stdout = std::io::stdout();
            let mut output = stdout.lock();
            Vm::new(&mut output)
                .execute(&compilation.module)
                .map_err(|error| {
                    eprintln!("[ERROR LXT-VM] {error}");
                    3
                })?;
            Ok(())
        }
        Command::Check { source, flags } => {
            let (source_file, compilation) = compile_path(&source, &flags)?;
            emit_optional_details(&source_file, &compilation, &flags);
            emit_diagnostics(&source_file, &compilation.diagnostics, flags.fun_errors);
            if compilation.has_errors() {
                Err(1)
            } else {
                println!("OK: {} is valid", source.display());
                Ok(())
            }
        }
        Command::Explain { source, flags } => {
            let (source_file, compilation) = compile_path(&source, &flags)?;
            println!("Detection: {:#?}", compilation.detection);
            println!("AI invoked: {}", compilation.used_ai);
            println!("Repairs: {}", compilation.repairs.len());
            for repair in &compilation.repairs {
                let position = source_file.position(repair.span.start);
                println!(
                    "  {} at {}:{} {:?} risk={:?}: {:?} -> {:?}",
                    repair.id,
                    position.line,
                    position.column,
                    repair.repair_type,
                    repair.semantic_risk,
                    repair.original_text,
                    repair.repaired_text
                );
            }
            println!(
                "IR:\n{}",
                serde_json::to_string_pretty(&compilation.module).map_err(|error| {
                    eprintln!("failed to encode IR: {error}");
                    2
                })?
            );
            emit_diagnostics(&source_file, &compilation.diagnostics, flags.fun_errors);
            if compilation.has_errors() {
                Err(1)
            } else {
                Ok(())
            }
        }
        Command::Models { command } => {
            match command {
                ModelsCommand::List => ai_support::list_models(),
                ModelsCommand::Test => {
                    ai_support::test_models().map_err(|()| 1)?;
                }
            }
            Ok(())
        }
        Command::Config { command } => {
            match command {
                ConfigCommand::Show => {
                    println!("repair.mode = aggressive");
                    println!("ai.local_endpoints = {}", ai_support::configured_count());
                    println!("ai.online_authorized = false");
                    println!("cache.enabled = true (override with --no-cache)");
                    println!("backend.run = tree-walking-vm");
                    println!("backend.build = versioned-ir-artifact");
                }
            }
            Ok(())
        }
        Command::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn compile_path(path: &Path, flags: &CompileFlags) -> Result<(SourceFile, Compilation), u8> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("lx") {
        eprintln!(
            "[WARNING LXT-SRC-001] {} does not use the .lx extension",
            path.display()
        );
    }
    let source = SourceFile::from_path(SourceId(0), path).map_err(|error| {
        eprintln!("[ERROR LXT-SRC-002] {error}");
        2
    })?;
    let options = flags.options();
    let mut key = CacheKeyInput::new(source.snapshot_hash(), options);
    if !flags.no_ai {
        key.model_identity = ai_support::cache_model_identity();
    }
    let cache_enabled = !flags.no_cache && std::env::var_os("LINXTS_DISABLE_CACHE").is_none();
    let cache = CompilationCache::default_for_user();
    if cache_enabled {
        match cache.load(&key) {
            Ok(Some(compilation)) => {
                if flags.verbose {
                    eprintln!("Cache: hit ({})", key.digest());
                }
                return Ok((source, compilation));
            }
            Ok(None) => {
                if flags.verbose {
                    eprintln!("Cache: miss ({})", key.digest());
                }
            }
            Err(error) => eprintln!("[WARNING LXT-CACHE-001] Ignoring cache entry: {error}"),
        }
    }

    let mut compilation = Compiler::compile(&source, options);
    ai_support::attempt_fallback(&source, &mut compilation, flags);
    if cache_enabled
        && !compilation.has_errors()
        && let Err(error) = cache.store(&key, &compilation)
    {
        eprintln!("[WARNING LXT-CACHE-002] Unable to store cache entry: {error}");
    }
    Ok((source, compilation))
}

fn emit_optional_details(source: &SourceFile, compilation: &Compilation, flags: &CompileFlags) {
    if flags.verbose {
        eprintln!("Detection: {:?}", compilation.detection);
        eprintln!("Repairs: {}", compilation.repairs.len());
        eprintln!("AI invoked: {}", compilation.used_ai);
    }
    if flags.show_ir {
        match serde_json::to_string_pretty(&compilation.module) {
            Ok(ir) => eprintln!("IR for {}:\n{ir}", source.name()),
            Err(error) => eprintln!("Unable to display IR: {error}"),
        }
    }
}

fn emit_diagnostics(source: &SourceFile, diagnostics: &[Diagnostic], fun_errors: bool) {
    for diagnostic in diagnostics {
        let label = match diagnostic.severity {
            Severity::Info => "INFO",
            Severity::Warning => "WARNING",
            Severity::Error => "ERROR",
        };
        eprintln!("\n[{label} {}]", diagnostic.code);
        eprintln!("File: {}", source.name());
        if let Some(span) = diagnostic.span {
            let position = source.position(span.start);
            eprintln!("Line: {}, Column: {}", position.line, position.column);
            if let Some(line) = source.line_text(position.line) {
                eprintln!("  {line}");
                eprintln!("  {}^", " ".repeat(position.column.saturating_sub(1)));
            }
        }
        eprintln!("Stage: {:?}", diagnostic.stage);
        eprintln!("{}", diagnostic.message);
        if let Some(chinese) = &diagnostic.message_zh_tw {
            eprintln!("繁體中文：{chinese}");
        }
        if let Some(suggestion) = &diagnostic.suggestion {
            eprintln!("Suggestion: {suggestion}");
        }
    }
    if fun_errors
        && diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        eprintln!("\nERR: 老哥的語法堪比一坨 💀");
    }
}

fn doctor() {
    println!("Linxts Doctor");
    println!(
        "[OK] target: {}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("[OK] deterministic compiler: available");
    println!("[OK] VM backend: available");
    println!("[TODO] native backend: unavailable");
    report_command("llama-server", "llama.cpp HTTP server");
    report_command("vulkaninfo", "Vulkan tooling");
    report_command("rocminfo", "ROCm tooling");
    println!("Note: availability in PATH does not prove that GPU inference works.");
}

fn report_command(command: &str, description: &str) {
    let available = std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(command).is_file())
    });
    println!(
        "[{}] {description}: {}",
        if available { "OK" } else { "INFO" },
        if available { "found" } else { "not found" }
    );
}
