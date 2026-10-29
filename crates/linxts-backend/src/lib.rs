//! Native backend that lowers validated Linxts IR to generated C and invokes a
//! system C toolchain. This is an explicit bootstrap backend, not Cranelift or
//! direct machine-code generation.

use linxts_ir::{
    BinaryOperator, Block, Expression, ExpressionKind, FormatSegment, Function, Literal, Module,
    StatementKind, UnaryOperator, Validator,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct NativeBuildOptions {
    pub c_compiler: PathBuf,
    pub optimize: bool,
}

impl Default for NativeBuildOptions {
    fn default() -> Self {
        Self {
            c_compiler: std::env::var_os("CC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("cc")),
            optimize: true,
        }
    }
}

#[derive(Debug, Default)]
pub struct CBootstrapBackend;

impl CBootstrapBackend {
    pub fn generate(module: &Module) -> Result<String, BackendError> {
        let errors = Validator::validate(module);
        if !errors.is_empty() {
            return Err(BackendError::InvalidIr(
                errors
                    .into_iter()
                    .map(|error| format!("{}: {}", error.code, error.message))
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        let mut generator = Generator::default();
        generator.emit_prelude();
        for function in &module.functions {
            generator.emit_prototype(function);
        }
        for function in &module.functions {
            generator.emit_function(function)?;
        }
        generator.emit_main(module)?;
        Ok(generator.output)
    }

    pub fn build(
        module: &Module,
        output: impl AsRef<Path>,
        options: &NativeBuildOptions,
    ) -> Result<(), BackendError> {
        let source = Self::generate(module)?;
        let output = output.as_ref();
        let parent = output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|source| BackendError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let temporary = temporary_source_path(parent);
        std::fs::write(&temporary, source).map_err(|source| BackendError::Io {
            path: temporary.clone(),
            source,
        })?;

        let mut command = Command::new(&options.c_compiler);
        command
            .arg("-std=c11")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-Werror")
            .arg("-Wno-unused-function");
        command.arg(if options.optimize { "-O2" } else { "-O0" });
        command.arg(&temporary).arg("-o").arg(output);
        let result = command
            .output()
            .map_err(|source| BackendError::ToolUnavailable {
                tool: options.c_compiler.clone(),
                source,
            });
        let _ = std::fs::remove_file(&temporary);
        let result = result?;
        if !result.status.success() {
            return Err(BackendError::ToolFailed {
                tool: options.c_compiler.clone(),
                status: result.status.code(),
                stderr: bounded_text(&result.stderr, 16 * 1024),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("native backend rejected invalid IR: {0}")]
    InvalidIr(String),
    #[error("unsupported IR operation: {0}")]
    Unsupported(String),
    #[error("native backend I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("C compiler {tool} is unavailable: {source}")]
    ToolUnavailable {
        tool: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("C compiler {tool} failed with status {status:?}: {stderr}")]
    ToolFailed {
        tool: PathBuf,
        status: Option<i32>,
        stderr: String,
    },
}

#[derive(Debug, Default)]
struct Generator {
    output: String,
    indent: usize,
}

impl Generator {
    fn emit_prelude(&mut self) {
        self.output.push_str(PRELUDE);
    }

    fn emit_prototype(&mut self, function: &Function) {
        let name = function_name(&function.name);
        let parameters = if function.parameters.is_empty() {
            "void".to_owned()
        } else {
            function
                .parameters
                .iter()
                .map(|_| "LxtValue")
                .collect::<Vec<_>>()
                .join(", ")
        };
        writeln!(self.output, "static LxtValue {name}({parameters});")
            .expect("writing to String cannot fail");
    }

    fn emit_function(&mut self, function: &Function) -> Result<(), BackendError> {
        let parameters = if function.parameters.is_empty() {
            "void".to_owned()
        } else {
            function
                .parameters
                .iter()
                .map(|parameter| format!("LxtValue {}", variable_name(&parameter.name)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        writeln!(
            self.output,
            "\nstatic LxtValue {}({parameters}) {{",
            function_name(&function.name)
        )
        .expect("writing to String cannot fail");
        self.indent += 1;
        self.emit_block_contents(&function.body)?;
        self.line("return lxt_unit();");
        self.indent -= 1;
        self.output.push_str("}\n");
        Ok(())
    }

    fn emit_main(&mut self, module: &Module) -> Result<(), BackendError> {
        self.output.push_str("\nint main(void) {\n");
        self.indent += 1;
        self.emit_block_contents(&module.body)?;
        if module
            .functions
            .iter()
            .any(|function| function.name == "main")
        {
            self.line("(void)lxt_fn_6d61696e();");
        }
        self.line("return 0;");
        self.indent -= 1;
        self.output.push_str("}\n");
        Ok(())
    }

    fn emit_block_contents(&mut self, block: &Block) -> Result<(), BackendError> {
        for statement in &block.statements {
            match &statement.kind {
                StatementKind::Variable { name, value, .. } => {
                    let expression = self.expression(value)?;
                    self.line(&format!("LxtValue {} = {expression};", variable_name(name)));
                }
                StatementKind::Assignment { name, value } => {
                    let expression = self.expression(value)?;
                    self.line(&format!("{} = {expression};", variable_name(name)));
                }
                StatementKind::Print(value) => {
                    let expression = self.expression(value)?;
                    self.line(&format!("lxt_print({expression});"));
                }
                StatementKind::Expression(value) => {
                    let expression = self.expression(value)?;
                    self.line(&format!("(void)({expression});"));
                }
                StatementKind::Conditional {
                    condition,
                    then_block,
                    else_block,
                } => {
                    let condition = self.expression(condition)?;
                    self.line(&format!("if (lxt_bool({condition})) {{"));
                    self.indent += 1;
                    self.emit_block_contents(then_block)?;
                    self.indent -= 1;
                    if let Some(else_block) = else_block {
                        self.line("} else {");
                        self.indent += 1;
                        self.emit_block_contents(else_block)?;
                        self.indent -= 1;
                    }
                    self.line("}");
                }
                StatementKind::Loop { condition, body } => {
                    let condition = self.expression(condition)?;
                    self.line(&format!("while (lxt_bool({condition})) {{"));
                    self.indent += 1;
                    self.emit_block_contents(body)?;
                    self.indent -= 1;
                    self.line("}");
                }
                StatementKind::Return(value) => {
                    let expression = value
                        .as_ref()
                        .map(|value| self.expression(value))
                        .transpose()?
                        .unwrap_or_else(|| "lxt_unit()".to_owned());
                    self.line(&format!("return {expression};"));
                }
            }
        }
        Ok(())
    }

    fn expression(&self, expression: &Expression) -> Result<String, BackendError> {
        match &expression.kind {
            ExpressionKind::Literal(literal) => Ok(match literal {
                Literal::Integer(value) => format!("lxt_int({value}LL)"),
                Literal::String(value) => format!("lxt_string({})", c_string(value)),
                Literal::Boolean(value) => format!("lxt_boolean({})", i32::from(*value)),
                Literal::Unit => "lxt_unit()".to_owned(),
            }),
            ExpressionKind::Variable { name } => Ok(variable_name(name)),
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.expression(left)?;
                let right = self.expression(right)?;
                Ok(match operator {
                    BinaryOperator::Add => format!("lxt_add({left}, {right})"),
                    BinaryOperator::Subtract => format!("lxt_sub({left}, {right})"),
                    BinaryOperator::Multiply => format!("lxt_mul({left}, {right})"),
                    BinaryOperator::Divide => format!("lxt_div({left}, {right})"),
                    BinaryOperator::Remainder => format!("lxt_rem({left}, {right})"),
                    BinaryOperator::Equal => format!("lxt_eq({left}, {right})"),
                    BinaryOperator::NotEqual => format!("lxt_not(lxt_eq({left}, {right}))"),
                    BinaryOperator::Less => format!("lxt_cmp({left}, {right}, 0)"),
                    BinaryOperator::LessEqual => format!("lxt_cmp({left}, {right}, 1)"),
                    BinaryOperator::Greater => format!("lxt_cmp({left}, {right}, 2)"),
                    BinaryOperator::GreaterEqual => format!("lxt_cmp({left}, {right}, 3)"),
                    BinaryOperator::And => {
                        format!("(lxt_bool({left}) ? lxt_as_bool({right}) : lxt_boolean(0))")
                    }
                    BinaryOperator::Or => {
                        format!("(lxt_bool({left}) ? lxt_boolean(1) : lxt_as_bool({right}))")
                    }
                })
            }
            ExpressionKind::Unary { operator, operand } => {
                let operand = self.expression(operand)?;
                Ok(match operator {
                    UnaryOperator::Negate => format!("lxt_neg({operand})"),
                    UnaryOperator::Not => format!("lxt_not({operand})"),
                })
            }
            ExpressionKind::Call { callee, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|argument| self.expression(argument))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("{}({arguments})", function_name(callee)))
            }
            ExpressionKind::Format { segments } => {
                let mut formatted = "lxt_string(\"\")".to_owned();
                for segment in segments {
                    let next = match segment {
                        FormatSegment::Text(text) => format!("lxt_string({})", c_string(text)),
                        FormatSegment::Value(value) => {
                            format!("lxt_to_string({})", self.expression(value)?)
                        }
                    };
                    formatted = format!("lxt_add({formatted}, {next})");
                }
                Ok(formatted)
            }
            ExpressionKind::Error { message } => Err(BackendError::Unsupported(format!(
                "error node reached code generation: {message}"
            ))),
        }
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(text);
        self.output.push('\n');
    }
}

fn variable_name(name: &str) -> String {
    format!("lxt_v_{}", hex_name(name))
}

fn function_name(name: &str) -> String {
    format!("lxt_fn_{}", hex_name(name))
}

fn hex_name(name: &str) -> String {
    name.as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn c_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for byte in value.as_bytes() {
        write!(escaped, "\\x{byte:02x}").expect("writing to String cannot fail");
    }
    escaped.push('"');
    escaped
}

fn temporary_source_path(directory: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    directory.join(format!(".linxts-native-{}-{nonce}.c", std::process::id()))
}

fn bounded_text(bytes: &[u8], limit: usize) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(limit)]).into_owned()
}

const PRELUDE: &str = r#"#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef enum { LXT_INT, LXT_STRING, LXT_BOOL, LXT_UNIT } LxtTag;
typedef struct { LxtTag tag; long long integer; const char *string; } LxtValue;

static void lxt_fatal(const char *message) { fputs(message, stderr); fputc('\n', stderr); exit(70); }
static LxtValue lxt_int(long long value) { return (LxtValue){LXT_INT, value, NULL}; }
static LxtValue lxt_string(const char *value) { return (LxtValue){LXT_STRING, 0, value}; }
static LxtValue lxt_boolean(int value) { return (LxtValue){LXT_BOOL, value != 0, NULL}; }
static LxtValue lxt_unit(void) { return (LxtValue){LXT_UNIT, 0, NULL}; }
static LxtValue lxt_to_string(LxtValue value) {
    if (value.tag == LXT_STRING) return value;
    if (value.tag == LXT_UNIT) return lxt_string("");
    char *text = malloc(32); if (!text) lxt_fatal("LXT-NATIVE: allocation failed");
    if (value.tag == LXT_INT) snprintf(text, 32, "%lld", value.integer);
    else snprintf(text, 32, "%s", value.integer ? "true" : "false");
    return lxt_string(text);
}
static int lxt_bool(LxtValue value) { if (value.tag != LXT_BOOL) lxt_fatal("LXT-NATIVE: expected boolean"); return value.integer != 0; }
static LxtValue lxt_as_bool(LxtValue value) { return lxt_boolean(lxt_bool(value)); }
static long long lxt_integer(LxtValue value) { if (value.tag != LXT_INT) lxt_fatal("LXT-NATIVE: expected integer"); return value.integer; }
static LxtValue lxt_add(LxtValue a, LxtValue b) {
    if (a.tag == LXT_STRING && b.tag == LXT_STRING) {
        size_t left = strlen(a.string), right = strlen(b.string);
        char *joined = malloc(left + right + 1); if (!joined) lxt_fatal("LXT-NATIVE: allocation failed");
        memcpy(joined, a.string, left); memcpy(joined + left, b.string, right + 1); return lxt_string(joined);
    }
    long long left = lxt_integer(a), right = lxt_integer(b), result;
    if (__builtin_add_overflow(left, right, &result)) result = right > 0 ? LLONG_MAX : LLONG_MIN;
    return lxt_int(result);
}
static LxtValue lxt_sub(LxtValue a, LxtValue b) { long long x=lxt_integer(a), y=lxt_integer(b), r; if (__builtin_sub_overflow(x,y,&r)) r=y<0?LLONG_MAX:LLONG_MIN; return lxt_int(r); }
static LxtValue lxt_mul(LxtValue a, LxtValue b) { long long x=lxt_integer(a), y=lxt_integer(b), r; if (__builtin_mul_overflow(x,y,&r)) r=((x<0)^(y<0))?LLONG_MIN:LLONG_MAX; return lxt_int(r); }
static LxtValue lxt_div(LxtValue a, LxtValue b) { long long x=lxt_integer(a), y=lxt_integer(b); if(y==0) lxt_fatal("LXT-NATIVE: division by zero"); if(x==LLONG_MIN&&y==-1) return lxt_int(LLONG_MAX); return lxt_int(x/y); }
static LxtValue lxt_rem(LxtValue a, LxtValue b) { long long x=lxt_integer(a), y=lxt_integer(b); if(y==0) lxt_fatal("LXT-NATIVE: division by zero"); if(x==LLONG_MIN&&y==-1) return lxt_int(0); return lxt_int(x%y); }
static LxtValue lxt_eq(LxtValue a, LxtValue b) { if(a.tag!=b.tag) return lxt_boolean(0); if(a.tag==LXT_STRING) return lxt_boolean(strcmp(a.string,b.string)==0); return lxt_boolean(a.integer==b.integer); }
static LxtValue lxt_cmp(LxtValue a, LxtValue b, int op) { long long x=lxt_integer(a), y=lxt_integer(b); return lxt_boolean(op==0?x<y:op==1?x<=y:op==2?x>y:x>=y); }
static LxtValue lxt_neg(LxtValue value) { long long x=lxt_integer(value); return lxt_int(x==LLONG_MIN?LLONG_MAX:-x); }
static LxtValue lxt_not(LxtValue value) { return lxt_boolean(!lxt_bool(value)); }
static void lxt_print(LxtValue value) { switch(value.tag) { case LXT_INT: printf("%lld\n",value.integer); break; case LXT_STRING: puts(value.string); break; case LXT_BOOL: puts(value.integer?"true":"false"); break; case LXT_UNIT: putchar('\n'); break; } }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use linxts_ir::{Expression, Statement};

    #[test]
    fn generates_standalone_main() {
        let span = Default::default();
        let mut module = Module::new("hash");
        module.body.statements.push(Statement::new(
            StatementKind::Print(Expression::integer(1, span)),
            span,
        ));
        let generated = CBootstrapBackend::generate(&module).expect("generate C");
        assert!(generated.contains("int main(void)"));
        assert!(generated.contains("lxt_print(lxt_int(1LL));"));
    }
}
