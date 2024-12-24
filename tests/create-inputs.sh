#!/usr/bin/env bash

set -exuo pipefail

echo "Setting up test environment in $1 …"
mkdir -p $1/tests/inputs
cp -r tests/inputs/hello $1/tests/inputs/
cd $1/tests/inputs

# Hello Zip archive (small text files)
zip -r9 hello.zip hello/

# An archive with some junk in the front
echo "Some junk up front" | cat - hello.zip > hello-prefixed.zip

# Create a Zip64 archive (one with files too large for original 32-bit fields)
mkdir zip64
truncate -s 100M zip64/zero100
truncate -s 5G zip64/zero5000
zip -r9 zip64.zip zip64/
