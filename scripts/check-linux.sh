#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo test --locked
cargo build --locked
CAPTURE=/tmp/post-ronda-linux.ppm
xvfb-run -a env POST_RONDA_CAPTURE="$CAPTURE" POST_RONDA_THEME=dark \
    "${CARGO_TARGET_DIR:-target}/debug/post-ronda" examples/workspace
test -s "$CAPTURE"
printf 'Linux native GUI capture passed\n'
