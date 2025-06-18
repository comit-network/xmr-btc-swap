#!/bin/bash
set -eu

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <version>"
    exit 1
fi

VERSION=$1
TODAY=$(date +%Y-%m-%d)
echo "Bumping version to $VERSION"

# Using sed and assuming GNU sed syntax as this is for the github workflow.

# Update version in tauri.conf.json
sed -i 's/"version": "[^"]*"/"version": "'"$VERSION"'"/' src-tauri/tauri.conf.json

# Update version in Cargo.toml files
sed -i -E 's/^version = "[0-9]+\.[0-9]+\.[0-9]+"/version = "'"$VERSION"'"/' swap/Cargo.toml src-tauri/Cargo.toml

# Update changelog
sed -i "s/^## \\[Unreleased\\]/## [$VERSION] - $TODAY/" CHANGELOG.md
# Add a new [Unreleased] section at the top
sed -i '3i## [Unreleased]\n' CHANGELOG.md

echo "Updated all files to version $VERSION." 