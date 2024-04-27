#!/usr/bin/env just --justfile

# just manual: https://github.com/casey/just/#readme
REGISTRY := "hub.getbetter.ro"
PROJECT := "epok"
HUB := `echo ${DOCKER_HUB:-"hub-cache.getbetter.ro"}`
CACHE_BUST := `date +%Y-%m-%d:%H:%M:%S`

_default:
  @just --list

# Run all CI steps
ci:
  just check
  just test
  just udeps

# Release the kraken
release:
  cargo build --release --locked

# Run clippy on the sources
check:
  cargo clippy --locked -- -D warnings

# Run unit tests
test:
  cargo test --locked

# Find unused dependencies
udeps:
  RUSTC_BOOTSTRAP=1 cargo udeps --all-targets --backend depinfo

# Hash important files to make a project tag
_tag:
  #!/usr/bin/env bash
  git ls-files -s \
    docker src build.rs rust-toolchain.toml \
    Cargo.toml Cargo.lock \
    | git hash-object --stdin \
    | cut -c-20

# Put together the full docker image
_image:
  #!/usr/bin/env bash
  if [ ! -z ${EPOK_IMAGE+x} ]; then
    echo $EPOK_IMAGE; exit
  fi
  tag=$(just _tag)
  echo {{REGISTRY}}/{{PROJECT}}:$tag

# Pull the docker image
pull:
  docker pull `just _image`

# Release using docker
docker-release *DOCKER_ARGS="":
  # '-v $CARGO_HOME:/tmp/.cargo_home' to reuse the local cargo cache
  mkdir -p target
  docker build -t `just _image`-release \
    --build-arg HUB={{HUB}} \
    {{DOCKER_ARGS}} \
    -f docker/Dockerfile-release .

# Build the docker image
docker:
  docker build -t `just _image` \
    --build-arg HUB={{HUB}} \
    --build-arg CACHE_BUST={{CACHE_BUST}} \
    -f docker/Dockerfile .

# Push the docker image
push:
  docker push `just _image`

# Push the "latest" docker image
push-latest:
  docker tag `just _image` {{REGISTRY}}/{{PROJECT}}:latest
  docker push {{REGISTRY}}/{{PROJECT}}:latest
