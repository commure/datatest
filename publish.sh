#!/bin/sh

DIR=`dirname $0`

if [[ -z "${VER}" ]] ; then
    echo "Set VER variable to the version to release!"
    exit 1
fi

cargo install cargo-readme

pushd "${DIR}"
cargo clean
cargo readme --output README.md
cargo test --all

git tag --annotate --message "releasing version ${VER}" "v${VER}"
git push --tags
popd

pushd "${DIR}/datatest-derive"
cargo publish
popd

pushd "${DIR}"
cargo publish
popd

