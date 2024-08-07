#!/bin/bash

set -euxo pipefail

VERSION=0.11.1

mkdir bdk
stat ./target/debug/swap || exit 1
cp ./target/debug/swap bdk/swap-current
pushd bdk

echo "download swap $VERSION"
curl -L "https://github.com/comit-network/xmr-btc-swap/releases/download/${VERSION}/swap_${VERSION}_Linux_x86_64.tar" | tar xv

echo "create testnet wallet with $VERSION"
./swap --testnet --data-base-dir . --debug balance || exit 1
echo "check testnet wallet with this version"
./swap-current --testnet --data-base-dir . --debug balance || exit 1

echo "create mainnet wallet with $VERSION"
./swap --version || exit 1
./swap --data-base-dir . --debug balance || exit 1
echo "check mainnet wallet with this version"
./swap-current --version || exit 1
./swap-current --data-base-dir . --debug balance || exit 1

exit 0
