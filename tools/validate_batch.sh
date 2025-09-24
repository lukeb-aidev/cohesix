#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: validate_batch.sh v0.1
# Author: Lukas Bower
# Date Modified: 2029-01-26
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: tools/validate_batch.sh [options] [PATH ...]

Scan documentation files and ensure they contain the required metadata headers.

Options:
  -h, --help           Show this help message and exit
  --strict             Treat warnings as errors (non-compliance exits 2)
  --log-file PATH      Write validation log to PATH (defaults to TMPDIR-aware path)
  --extensions LIST    Comma-separated list of file extensions to validate (default: md,txt,adoc)

If no PATH is provided the script scans common documentation roots such as
workspace/docs and docs relative to the repository root.
USAGE
}

log() {
  printf '[validate-batch] %s\n' "$1"
}

warn() {
  printf '[validate-batch][warn] %s\n' "$1" >&2
}

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

STRICT=0
LOG_FILE=""
EXTENSIONS="md,txt,adoc"
TARGETS=()

while (($#)); do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --strict)
      STRICT=1
      shift
      ;;
    --log-file)
      if [[ $# -lt 2 ]]; then
        warn "--log-file requires a path argument"
        exit 1
      fi
      LOG_FILE="$2"
      shift 2
      ;;
    --extensions)
      if [[ $# -lt 2 ]]; then
        warn "--extensions requires a comma-separated list"
        exit 1
      fi
      EXTENSIONS="$2"
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -* )
      warn "Unknown option: $1"
      usage
      exit 1
      ;;
    *)
      TARGETS+=("$1")
      shift
      ;;
  esac
done

if (($#)); then
  for arg in "$@"; do
    TARGETS+=("$arg")
  done
fi

if [[ ${#TARGETS[@]} -eq 0 ]]; then
  if [[ -d workspace/docs ]]; then
    TARGETS+=("workspace/docs")
  fi
  if [[ -d docs ]]; then
    TARGETS+=("docs")
  fi
fi

if [[ ${#TARGETS[@]} -eq 0 ]]; then
  warn "No documentation paths found to validate"
  exit 1
fi

select_tmp_root() {
  local candidate
  for candidate in "${COHESIX_TRACE_TMP:-}" "${COHESIX_ENS_TMP:-}" "${TMPDIR:-}"; do
    if [[ -n "$candidate" ]]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  mktemp -d
}

TMP_ROOT="$(select_tmp_root)"
if [[ -n "$LOG_FILE" ]]; then
  mkdir -p "$(dirname "$LOG_FILE")"
else
  mkdir -p "$TMP_ROOT/cohesix_batch_logs"
  LOG_FILE="$TMP_ROOT/cohesix_batch_logs/validate_batch_$(date +%Y%m%d_%H%M%S).log"
fi

exec > >(tee "$LOG_FILE") 2>&1
log "Validation log: $LOG_FILE"

IFS=',' read -r -a EXT_ARRAY <<< "$EXTENSIONS"

mapfile -t FILES < <(
  for target in "${TARGETS[@]}"; do
    if [[ -d "$target" ]]; then
      while IFS= read -r candidate; do
        for ext in "${EXT_ARRAY[@]}"; do
          if [[ "$candidate" == *".${ext}" ]]; then
            printf '%s\n' "$candidate"
            break
          fi
        done
      done < <(find "$target" -type f -print)
    elif [[ -f "$target" ]]; then
      printf '%s\n' "$target"
    else
      warn "Skipping missing path: $target"
    fi
  done
)

if [[ ${#FILES[@]} -eq 0 ]]; then
  warn "No documentation files matched the provided paths"
  exit 1
fi

python3 - "$STRICT" "${FILES[@]}" <<'PY'
import re
import sys
from pathlib import Path

STRICT = bool(int(sys.argv[1]))
paths = [Path(p) for p in sys.argv[2:]]

HEADER_FIELDS = (
    "CLASSIFICATION",
    "Filename",
    "Author",
    "Date Modified",
)

def normalize_comment(line: str) -> str:
    stripped = line.strip()
    if not stripped:
        return ""
    if stripped.startswith("//"):
        return stripped[2:].strip()
    if stripped.startswith("#"):
        return stripped[1:].strip()
    if stripped.startswith(";"):
        return stripped[1:].strip()
    return ""

errors = []
checks = 0

for path in paths:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except UnicodeDecodeError:
        errors.append(f"{path}: unable to decode as UTF-8")
        continue
    header = []
    for line in lines[:12]:
        comment = normalize_comment(line)
        if not comment and line.strip():
            break
        if comment:
            header.append(comment)
    missing = []
    values = {}
    for field in HEADER_FIELDS:
        for comment in header:
            if comment.startswith(f"{field}:"):
                values[field] = comment.split(":", 1)[1].strip()
                break
        else:
            missing.append(field)
    if missing:
        errors.append(f"{path}: missing header fields: {', '.join(missing)}")
        continue
    filename_value = values.get("Filename", "")
    if filename_value:
        token = filename_value.split()[0]
        actual_name = path.name
        if token != actual_name:
            errors.append(
                f"{path}: Filename header '{token}' does not match actual file '{actual_name}'"
            )
    author = values.get("Author", "")
    if author and author != "Lukas Bower":
        errors.append(f"{path}: Author header should be 'Lukas Bower' (found '{author}')")
    date = values.get("Date Modified", "")
    if date and not re.fullmatch(r"\d{4}-\d{2}-\d{2}", date):
        errors.append(f"{path}: Date Modified should use YYYY-MM-DD (found '{date}')")
    classification = values.get("CLASSIFICATION", "")
    if classification not in {"COMMUNITY", "PRIVATE"}:
        errors.append(
            f"{path}: CLASSIFICATION must be COMMUNITY or PRIVATE (found '{classification}')"
        )
    checks += 1

for msg in errors:
    print(f"[validate-batch][error] {msg}", file=sys.stderr)

print(f"[validate-batch] Checked {checks} file(s); {len(errors)} issue(s) detected")
if errors:
    sys.exit(2 if STRICT else 1)
PY
