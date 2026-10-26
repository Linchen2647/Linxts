#!/usr/bin/env bash
set -euo pipefail

required=(
  LINXTS_LLAMA_3B_URL
  LINXTS_LLAMA_3B_MODEL
  LINXTS_LLAMA_3B_GGUF
  LINXTS_LLAMA_7B_URL
  LINXTS_LLAMA_7B_MODEL
  LINXTS_LLAMA_7B_GGUF
  LINXTS_LLAMA_SERVER_VERSION
  LINXTS_LLAMA_BACKEND
)
for name in "${required[@]}"; do
  if [[ -z "${!name:-}" ]]; then
    printf 'missing required environment variable: %s\n' "$name" >&2
    exit 2
  fi
done
unset LINXTS_LLAMA_LARGE_URL LINXTS_LLAMA_LARGE_MODEL

for endpoint in "$LINXTS_LLAMA_3B_URL" "$LINXTS_LLAMA_7B_URL"; do
  case "$endpoint" in
    http://127.0.0.1:*|http://localhost:*|http://\[::1\]:*) ;;
    *)
      printf 'refusing non-loopback endpoint: %s\n' "$endpoint" >&2
      exit 2
      ;;
  esac
done

for model in "$LINXTS_LLAMA_3B_GGUF" "$LINXTS_LLAMA_7B_GGUF"; do
  if [[ ! -f "$model" ]]; then
    printf 'model file does not exist: %s\n' "$model" >&2
    exit 2
  fi
done
repeats=${LINXTS_ACCEPTANCE_REPEATS:-3}
if [[ ! "$repeats" =~ ^[0-9]+$ ]] || (( repeats < 3 )); then
  printf 'LINXTS_ACCEPTANCE_REPEATS must be an integer of at least 3\n' >&2
  exit 2
fi

for command in cargo git rustc sha256sum; do
  if ! command -v "$command" >/dev/null 2>&1; then
    printf 'required command is unavailable: %s\n' "$command" >&2
    exit 2
  fi
done

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
fixture="$root/tests/fixtures/ai/real-escalation.lx"
expected="$root/tests/fixtures/ai/real-escalation.stdout"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
out_dir=${LINXTS_ACCEPTANCE_OUT:-"$root/target/real-model-acceptance/$stamp"}
if [[ -e "$out_dir" ]]; then
  printf 'refusing to overwrite existing evidence directory: %s\n' "$out_dir" >&2
  exit 2
fi
mkdir -p "$out_dir"
printf 'Acceptance evidence: %s\n' "$out_dir"
trap 'printf "Acceptance did not pass; retained evidence in %s\n" "$out_dir" >&2' ERR

cargo build --manifest-path "$root/Cargo.toml" --locked --offline -p linxts-cli
binary="$root/target/debug/linxts"

sha_3b=$(sha256sum "$LINXTS_LLAMA_3B_GGUF" | cut -d ' ' -f 1)
sha_7b=$(sha256sum "$LINXTS_LLAMA_7B_GGUF" | cut -d ' ' -f 1)
fixture_sha=$(sha256sum "$fixture" | cut -d ' ' -f 1)
{
  printf 'timestamp_utc=%s\n' "$stamp"
  printf 'git_commit=%s\n' "$(git -C "$root" rev-parse HEAD)"
  printf 'git_dirty=%s\n' "$(if [[ -n "$(git -C "$root" status --porcelain)" ]]; then printf true; else printf false; fi)"
  printf 'rustc=%s\n' "$(rustc --version)"
  printf 'cargo=%s\n' "$(cargo --version)"
  printf 'kernel=%s\n' "$(uname -srmo)"
  printf 'llama_server_version=%s\n' "$LINXTS_LLAMA_SERVER_VERSION"
  printf 'llama_backend=%s\n' "$LINXTS_LLAMA_BACKEND"
  printf 'model_3b_id=%s\n' "$LINXTS_LLAMA_3B_MODEL"
  printf 'model_3b_sha256=%s\n' "$sha_3b"
  printf 'model_7b_id=%s\n' "$LINXTS_LLAMA_7B_MODEL"
  printf 'model_7b_sha256=%s\n' "$sha_7b"
  printf 'fixture_sha256=%s\n' "$fixture_sha"
  printf 'temperature=0.0\n'
  printf 'retries=0\n'
  printf 'repetitions=%s\n' "$repeats"
  printf 'timeout_seconds=%s\n' "${LINXTS_AI_TIMEOUT_SECS:-180}"
} >"$out_dir/manifest.txt"
cp "$fixture" "$expected" "$out_dir/"

if ! "$binary" models test >"$out_dir/health.stdout" 2>"$out_dir/health.stderr"; then
  printf 'model health check failed\n' >&2
  exit 1
fi

for ((run_index = 1; run_index <= repeats; run_index++)); do
  explain_stdout="$out_dir/explain-$run_index.stdout"
  explain_stderr="$out_dir/explain-$run_index.stderr"
  run_stdout="$out_dir/run-$run_index.stdout"
  run_stderr="$out_dir/run-$run_index.stderr"

  if ! LINXTS_AI_RETRIES=0 LINXTS_AI_TIMEOUT_SECS="${LINXTS_AI_TIMEOUT_SECS:-180}" \
    "$binary" explain "$fixture" --offline --no-cache \
    >"$explain_stdout" 2>"$explain_stderr"; then
    printf 'Linxts explain failed on repetition %s\n' "$run_index" >&2
    exit 1
  fi

  if ! grep -Fq 'AI invoked: true' "$explain_stdout"; then
    printf 'AI fallback was not invoked on repetition %s\n' "$run_index" >&2
    exit 1
  fi
  attempt_3b=$(grep -F 'Local3B/local3b#1:' "$explain_stderr" || true)
  if [[ -z "$attempt_3b" ]]; then
    printf 'no completed 3B inference attempt was recorded on repetition %s\n' "$run_index" >&2
    exit 1
  fi
  if [[ "$attempt_3b" == *'valid candidate'* ]]; then
    printf '3B produced a valid candidate on repetition %s; escalation was not demonstrated\n' "$run_index" >&2
    exit 1
  fi
  if ! grep -Fq 'Local7B/local7b#1: valid candidate' "$explain_stderr"; then
    printf '7B did not produce the accepted valid candidate on repetition %s\n' "$run_index" >&2
    exit 1
  fi
  if ! grep -Fq "accepted validated IR from $LINXTS_LLAMA_7B_MODEL" "$explain_stderr"; then
    printf 'accepted provider does not match the configured 7B model on repetition %s\n' "$run_index" >&2
    exit 1
  fi

  if ! LINXTS_AI_RETRIES=0 LINXTS_AI_TIMEOUT_SECS="${LINXTS_AI_TIMEOUT_SECS:-180}" \
    "$binary" run "$fixture" --offline --no-cache \
    >"$run_stdout" 2>"$run_stderr"; then
    printf 'accepted candidate did not execute successfully on repetition %s\n' "$run_index" >&2
    exit 1
  fi
  if ! cmp -s "$expected" "$run_stdout"; then
    printf 'runtime output differs from expectation on repetition %s\n' "$run_index" >&2
    exit 1
  fi
done

printf 'result=PASS\n' >>"$out_dir/manifest.txt"
trap - ERR
printf 'PASS: real 3B inference failed validation, 7B produced validated IR, and output matched.\n'
