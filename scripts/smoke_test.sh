#!/bin/bash
set -e

export DEV_DISABLE_SNFORGE_STD_DEPENDENCY=true

RPC_URL="$1"
SNFORGE_PATH="$2"
SNCAST_PATH="$3"
REPO_URL="$4"
REVISION="$5"
VERSION="$6"

# Check forge

$SNFORGE_PATH init my_project
pushd my_project || exit
sed -i.bak "/snforge_std/ s/\(snforge_std = \).*/\1{ version = \"${VERSION}\", registry = \"https:\/\/scarbs.dev\/\" }/" Scarb.toml
rm Scarb.toml.bak 2> /dev/null
$SNFORGE_PATH test || exit
popd || exit

# Check cast

if ! $SNCAST_PATH call \
    --url "$RPC_URL" \
    --contract-address 0x06b248bde9ce00d69099304a527640bc9515a08f0b49e5168e2096656f207e1d \
    --function "get" --calldata 0x1 | grep -q $'command: call\nresponse: [0x0]'; then
  exit 1
fi
