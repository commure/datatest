#!/bin/sh

DIR=`dirname $0`

cargo install cargo-readme

pushd "${DIR}"
cargo readme --output README.md
cargo test
popd

pushd "${DIR}/datatest-derive"
cargo publish --dry-run
popd

pushd "${DIR}"
cargo publish --dry-run
popd

pushd "${DIR}/datatest-derive"
cargo publish
popd

pushd "${DIR}"
cargo publish
popd

