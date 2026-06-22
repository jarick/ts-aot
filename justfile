set shell := ["powershell.exe", "-NoLogo", "-Command"]

_default:
    @just --list

check:
    cargo check --workspace --all-targets --locked

test:
    cargo test --workspace --locked

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

build:
    cargo build --workspace --locked