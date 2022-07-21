#!/usr/bin/env just --justfile

# just manual: https://github.com/casey/just/#readme
REGISTRY := "registry-np.storage-system.svc.k8s.local:5000"
PROJECT := "epok"
HUB := "hub-cache.getbetter.ro"
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
    Cargo Cargo.lock \
    | git hash-object --stdin \
    | cut -c-20

# Put together the full docker image
_image:
  #!/usr/bin/env bash
  tag=$(just _tag)
  echo {{REGISTRY}}/{{PROJECT}}:$tag

# Pull the docker image
pull:
  #!/usr/bin/env bash
  image=$(just _image)
  docker pull $image

# Build the docker image
docker:
  #!/usr/bin/env bash
  echo {{CACHE_BUST}}
  image=$(just _image)
  docker build -t $image --build-arg HUB={{HUB}} --build-arg CACHE_BUST={{CACHE_BUST}} -f docker/Dockerfile .

# Push the docker image
push:
  #!/usr/bin/env bash
  image=$(just _image)
  docker push $image

# Push the "latest" docker image
push-latest:
  #!/usr/bin/env bash
  image=$(just _image)
  docker tag $image {{REGISTRY}}/{{PROJECT}}:latest
  docker push {{REGISTRY}}/{{PROJECT}}:latest
