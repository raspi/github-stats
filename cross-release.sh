#!/bin/bash

# Uses cross, See: https://github.com/cross-rs/cross

EXE="github-stats"

CROSSBIN="$HOME/.cargo/bin/cross"
CROSSARGS="build --release"

# https://github.com/cross-rs/cross#supported-targets
ARCHS="x86_64-unknown-linux-gnu x86_64-unknown-netbsd powerpc64-unknown-linux-gnu powerpc64le-unknown-linux-gnu aarch64-unknown-linux-gnu arm-unknown-linux-gnueabi"

for t in $ARCHS
do
  echo "ARCH: $t -------------------------------------------"
  $CROSSBIN $CROSSARGS --target "$t"

  exe=$EXE
  if [ "$t" == "x86_64-pc-windows-gnu" ]; then
    exe="${exe}.exe"
  fi

  upx -9 "target/$t/release/$exe"

  echo ""
  echo "--------------------------------------------"
  echo ""
done

# copy common files
for t in $ARCHS
do
  cp LICENSE "target/$t/release"
  cp README.md "target/$t/release"
  cp config.example.toml "target/$t/release"
done

# Get version from compiled release
VERSION=$(target/x86_64-unknown-linux-gnu/release/$EXE --version | cut -d' ' -f2)

mkdir "release/v$VERSION"

for t in $ARCHS
do
  pushd "target/$t/release" || return
  exe=$EXE
  if [ "$t" == "x86_64-pc-windows-gnu" ]; then
    exe="${exe}.exe"
  fi

  tar --numeric-owner --owner=0 --group=0 -zcf "../../../release/v$VERSION/$EXE-v$VERSION-$t.tar.gz" config.example.toml LICENSE README.md "$exe"
  popd || return
done

pushd "release/v$VERSION" || return
sha256sum *.tar.gz > checksums.sha256
popd || return
