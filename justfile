#!/usr/bin/env just --justfile

# just manual: https://github.com/casey/just/#readme

_default:
  @just --list

# Release the kraken
release:
  cargo build --release

# Run clippy on the sources
check:
  cargo clippy --locked -- -D warnings

# Find unused dependencies
udeps:
  RUSTC_BOOTSTRAP=1 cargo udeps --all-targets --backend depinfo
