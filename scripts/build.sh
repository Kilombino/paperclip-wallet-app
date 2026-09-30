#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
: "${IN_NIX_SHELL:?Run this script with nix develop --command bash scripts/build.sh}"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
BARK_WEB_DIST="$PWD/web" cargo build --locked -p bark-cli --features barkd-web-ui --bins
