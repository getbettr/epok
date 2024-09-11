#!/usr/bin/env just --justfile

# just manual: https://github.com/casey/just/#readme
REGISTRY := `echo ${REGISTRY:-"hub.getbetter.ro"}`
PROJECT := "epok"
HUB := `echo ${DOCKER_HUB:-"hub-cache.getbetter.ro"}`
CACHE_BUST := `date +%Y-%m-%d:%H:%M:%S`
DEFAULT_RELEASE := "patch"

_default:
  @just --list

# Run all CI steps
ci:
  just check
  just test
  just udeps

# Build the kraken
build:
  cargo build --release --locked

# Release the kraken
release type=DEFAULT_RELEASE:
  #!/usr/bin/env bash
  cargo release version {{type}} -x --no-confirm
  cargo release commit -x --no-confirm
  cargo release -x --no-confirm || {
    echo -e "Release failed, removing latest tag and rewinding to HEAD~1..."
    git tag --delete $(git tag -l | sort -r | head -n 1)
    git reset --hard HEAD~1
    exit 1
  }

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
  #!/usr/bin/env bash
  mkdir -p target/release
  image="$(just _image)-release"
  docker build -t $image \
    --build-arg HUB={{HUB}} \
    {{DOCKER_ARGS}} \
    -f docker/Dockerfile-release .
  container=$(docker create $image)
  docker cp $container:/epok/target/release/epok target/release/
  docker cp $container:/epok/target/release/epok-clean target/release/
  docker rm -f $container


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
