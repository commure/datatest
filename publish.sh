#!/bin/sh

DIR=`dirname $0`

if [[ -z "${VER}" ]] ; then
    echo "Set VER variable to the version to release!"
    exit 1
fi

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


pushd "${DIR}"
git tag --annotate --message "releasing version X.Y.Z" "vX.Y.Z"
git push --tags
popd

pushd "${DIR}/datatest-derive"
cargo publish
popd

pushd "${DIR}"
cargo publish
popd

