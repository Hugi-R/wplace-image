#!/bin/bash
set -e

# Build the project
cargo build --release

# Run the smoke test
rm -rf smoketest_output
mkdir -p smoketest_output
./target/release/wimage_cli convert wimage/src/testdata/00.png smoketest_output/00.wimg
./target/release/wimage_cli decompress smoketest_output/00.wimg smoketest_output/00_decompressed.png
./target/release/wimage_cli downscale smoketest_output/00.wimg smoketest_output/00x2.wimg -f 2
./target/release/wimage_cli decompress smoketest_output/00x2.wimg smoketest_output/00x2_decompressed.png