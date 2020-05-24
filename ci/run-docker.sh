#!/usr/bin/env sh

# Small script to run tests for a target (or all targets) inside all the
# respective docker images.

set -e

echo "${HOME}"
pwd

TARGET="${1}"
shift

echo "Building docker container for target $target"

# use -f so we can use ci/ as build context
image_tag=test-"$TARGET"
docker build -t "$image_tag" -f "ci/docker/${TARGET}/Dockerfile" ci/
mkdir -p target

set -x

docker run \
  --rm \
  --user "$(id -u)":"$(id -g)" \
  --env CARGO_HOME=/cargo \
  --env CARGO_TARGET_DIR=/checkout/target \
  --volume "$(dirname "$(dirname "$(command -v cargo)")")":/cargo \
  --volume "$(rustc --print sysroot)":/rust:ro \
  --volume "$(pwd)":/checkout:ro \
  --volume "$(pwd)"/target:/checkout/target \
  --init \
  --workdir /checkout \
  "$image_tag" \
  sh -c "HOME=/tmp PATH=\$PATH:/rust/bin exec cargo --locked test --target ${TARGET} $@"
