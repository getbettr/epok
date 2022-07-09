#!/usr/bin/env just --justfile

# just manual: https://github.com/casey/just/#readme

_default:
  @just --list

# Releases the kraken
release:
  cargo build --release

# Runs clippy on the sources
check:
  cargo clippy --locked -- -D warnings
