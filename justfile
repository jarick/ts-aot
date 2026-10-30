set shell := ["powershell.exe", "-NoLogo", "-Command"]

_default:
    @just --list

python := if os() == "windows" { "python" } else { "python3" }

_fmt_cmd := "cargo fmt --all -- --check"
_clippy_cmd := "cargo clippy --workspace --all-targets --locked -- -D warnings"
_check_cmd := "cargo check --workspace --all-targets --locked"

check:
    {{_fmt_cmd}}
    {{_clippy_cmd}}
    {{_check_cmd}}

test:
    cargo test --workspace --locked

fmt:
    cargo fmt --all

fmt-check:
    {{_fmt_cmd}}

clippy:
    {{_clippy_cmd}}

build:
    cargo build --workspace --locked

test262-deps:
    {{python}} -m pip install -r tests/requirements.txt

test262:
    cargo build --release --locked -p ts-aot-driver --bin ts-aot
    {{python}} tests/test262_harness.py --max 0 --emit mir --workers 12
