#!/bin/bash

set -e

rm -rf target/deb
PACKNAME=$1
VERSION=`cargo --frozen metadata --no-deps --format-version 1 | jq -r '.packages.[] | select(.name=="'${PACKNAME}'").version'`
ARCHNAME=$2

WORKDIR=target/deb/${PACKNAME}_$VERSION-1_$ARCHNAME
mkdir -p $WORKDIR
mkdir $WORKDIR/DEBIAN
cat <<EOF > $WORKDIR/DEBIAN/control
Package: $PACKNAME
Version: $VERSION
Maintainer: valkyrie_pilot <valk@randomairborne.dev>
Depends: libc6
Architecture: $ARCHNAME
Homepage: https://github.com/randomairborne/godsvagn
Description: Apt repo host with github integration
EOF
mkdir -p $WORKDIR/usr/bin/
cp target/release/$PACKNAME $WORKDIR/usr/bin/