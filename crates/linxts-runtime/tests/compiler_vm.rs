use linxts_compiler::{CompileOptions, Compiler};
use linxts_runtime::Vm;
use linxts_source::{SourceFile, SourceId};

fn execute(text: &str) -> String {
    let source = SourceFile::from_text(SourceId(0), "integration.lx", text);
    let compilation = Compiler::compile(&source, CompileOptions::default());
    assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);
    let mut output = Vec::new();
    Vm::new(&mut output)
        .execute(&compilation.module)
        .expect("program should execute");
    String::from_utf8(output).expect("VM output should be UTF-8")
}

#[test]
fn print_variants_have_expected_output() {
    assert_eq!(execute("PRINT 1\n"), "1\n");
    assert_eq!(execute("print 1\n"), "1\n");
    assert_eq!(execute("Print(\"Hello\")\n"), "Hello\n");
}

#[test]
fn language_hints_and_repair_have_expected_output() {
    assert_eq!(execute("^這是python\nprint(\"Hello\")\n"), "Hello\n");
    assert_eq!(execute("^javascript\nconsole.log(\"Hello\"\n"), "Hello\n");
    assert_eq!(
        execute("^rust\nfn main() {\n  println!(\"Hello\")\n"),
        "Hello\n"
    );
    assert_eq!(
        execute("^C語言\nint main() {\n  printf(\"Hello from C\");\n  return 0;\n"),
        "Hello from C\n"
    );
}

#[test]
fn natural_sum_has_expected_output() {
    assert_eq!(execute("幫我算出1到100的總和\n"), "5050\n");
}

#[test]
fn cross_language_formatting_shares_variables() {
    let source = "^python\nx = 7\nprint(f\"py={x}\")\n^javascript\nconsole.log(`js=${x}`)\n^rust\nprintln!(\"rs={}\", x)\n^c\nprintf(\"c=%d\", x)\n";
    assert_eq!(execute(source), "py=7\njs=7\nrs=7\nc=7\n");
}

#[test]
fn python_indentation_and_range_execute() {
    let source = "^python\ntotal = 0\nfor i in range(1, 5):\n    total = total + i\nprint(f\"total={total}\")\nfor j in range(3, 0, -1):\n    print(j)\n";
    assert_eq!(execute(source), "total=10\n3\n2\n1\n");
}

#[test]
fn aggressive_repair_preserves_python_indentation() {
    let source = "^python\nfor i in range(2):\n    print(\"x\n";
    assert_eq!(execute(source), "x\nx\n");
}

#[test]
fn mixed_language_shares_values() {
    let source =
        "^python\nx = 10\n^javascript\nconsole.log(x)\n^自然語言\n把 x 乘以 5，然後顯示結果\n";
    assert_eq!(execute(source), "10\n50\n");
}
