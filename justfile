set shell := ["powershell.exe", "-NoLogo", "-Command"]

_default:
    @just --list

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