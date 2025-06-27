#!/bin/bash

# UnstoppableSwap Flatpak Build and Deploy Script
# Usage: ./flatpak-build.sh [--push] [--branch BRANCH] [--no-gpg]
# Example: ./flatpak-build.sh --push --branch gh-pages

set -e

PUSH_FLAG=""
BRANCH="gh-pages"
GPG_SIGN=""
NO_GPG_FLAG=""
REPO_DIR="flatpak-repo"
TEMP_DIR="$(mktemp -d)"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --push)
            PUSH_FLAG="--push"
            shift
            ;;
        --branch)
            BRANCH="$2"
            shift 2
            ;;
        --no-gpg)
            NO_GPG_FLAG="--no-gpg"
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--push] [--branch BRANCH] [--no-gpg]"
            exit 1
            ;;
    esac
done

# Function to list available GPG keys
list_gpg_keys() {
    echo "📋  Available GPG keys:"
    gpg --list-secret-keys --keyid-format=long 2>/dev/null | grep -E "^(sec|uid)" | while IFS= read -r line; do
        if [[ $line =~ ^sec ]]; then
            key_info=$(echo "$line" | awk '{print $2}')
            echo "   🔑  Key: $key_info"
        elif [[ $line =~ ^uid ]]; then
            uid=$(echo "$line" | sed 's/uid[[:space:]]*\[[^]]*\][[:space:]]*//')
            echo "      👤  $uid"
            echo ""
        fi
    done
}

# Function to get GPG key selection
select_gpg_key() {
    if ! command -v gpg &> /dev/null; then
        echo "❌  GPG is not installed. Install with: sudo apt install gnupg"
        exit 1
    fi

    local keys=($(gpg --list-secret-keys --keyid-format=long 2>/dev/null | grep "^sec" | awk '{print $2}' | cut -d'/' -f2))

    if [ ${#keys[@]} -eq 0 ]; then
        echo "🔑  No GPG keys found."
        echo ""
        read -p "Would you like to import a GPG key? [y/N]: " import_key

        if [[ $import_key =~ ^[Yy]$ ]]; then
            import_gpg_key
            select_gpg_key
        else
            echo "⚠️   Proceeding without GPG signing (not recommended for production)"
            GPG_SIGN=""
            return
        fi
    else
        echo ""
        list_gpg_keys

        echo "Please select a GPG key for signing:"
        for i in "${!keys[@]}"; do
            local key_id="${keys[i]}"
            local user_info=$(gpg --list-secret-keys --keyid-format=long "$key_id" 2>/dev/null | grep "^uid" | head -1 | sed 's/uid[[:space:]]*\[[^]]*\][[:space:]]*//')
            echo "   $((i+1))) ${key_id} - ${user_info}"
        done
        echo "   $((${#keys[@]}+1))) Skip GPG signing"
        echo "   $((${#keys[@]}+2))) Import a new key"
        echo ""

        while true; do
            read -p "Enter your choice [1-$((${#keys[@]}+2))]: " choice

            if [[ $choice =~ ^[0-9]+$ ]] && [ $choice -ge 1 ] && [ $choice -le $((${#keys[@]}+2)) ]; then
                if [ $choice -eq $((${#keys[@]}+1)) ]; then
                    echo "⚠️   Proceeding without GPG signing"
                    GPG_SIGN=""
                    break
                elif [ $choice -eq $((${#keys[@]}+2)) ]; then
                    import_gpg_key
                    select_gpg_key
                    break
                else
                    GPG_SIGN="${keys[$((choice-1))]}"
                    local selected_user=$(gpg --list-secret-keys --keyid-format=long "$GPG_SIGN" 2>/dev/null | grep "^uid" | head -1 | sed 's/uid[[:space:]]*\[[^]]*\][[:space:]]*//')
                    echo "✅  Selected key: $GPG_SIGN - $selected_user"
                    break
                fi
            else
                echo "❌  Invalid choice. Please enter a number between 1 and $((${#keys[@]}+2))"
            fi
        done
    fi
}

# Function to import GPG key
import_gpg_key() {
    echo ""
    echo "🔑  GPG Key Import"
    echo "=================="
    echo "📝  Please paste your GPG private key below."
    echo "   (Start with -----BEGIN PGP PRIVATE KEY BLOCK----- and end with -----END PGP PRIVATE KEY BLOCK-----)"
    echo "   Press Ctrl+D when finished:"
    echo ""

    local temp_key_file=$(mktemp)
    cat > "$temp_key_file"

    echo ""
    echo "🔄  Importing key..."

    if gpg --import "$temp_key_file" 2>/dev/null; then
        echo "✅  GPG key imported successfully!"
    else
        echo "❌  Failed to import GPG key. Please check the format and try again."
        rm -f "$temp_key_file"
        exit 1
    fi

    rm -f "$temp_key_file"
}

# Check requirements
if ! command -v flatpak-builder &> /dev/null; then
    echo "❌  flatpak-builder is required but not installed"
    echo "Install with: sudo apt install flatpak-builder (Ubuntu/Debian)"
    echo "             sudo dnf install flatpak-builder (Fedora)"
    exit 1
fi

if ! command -v git &> /dev/null; then
    echo "❌  git is required but not installed"
    exit 1
fi

if ! command -v jq &> /dev/null; then
    echo "❌  jq is required but not installed"
    echo "Install with: sudo apt install jq (Ubuntu/Debian)"
    echo "             sudo dnf install jq (Fedora)"
    exit 1
fi

# Get repository info
REPO_URL=$(git remote get-url origin 2>/dev/null || echo "")
if [[ $REPO_URL =~ github\.com[:/]([^/]+)/([^/.]+) ]]; then
    GITHUB_USER="${BASH_REMATCH[1]}"
    REPO_NAME="${BASH_REMATCH[2]}"
else
    echo "❌  Could not determine GitHub repository info"
    echo "Make sure you're in a Git repository with a GitHub origin"
    exit 1
fi

PAGES_URL="https://${GITHUB_USER}.github.io/${REPO_NAME}"

echo "🏗️   Building Flatpak for UnstoppableSwap..."
echo "📁  Repository: ${GITHUB_USER}/${REPO_NAME}"
echo "🌐  Pages URL: ${PAGES_URL}"
echo ""

# Handle GPG key selection
if [ "$NO_GPG_FLAG" != "--no-gpg" ]; then
    echo "🔐  GPG Signing Setup"
    echo "==================="
    echo "For security, it's highly recommended to sign your Flatpak repository with GPG."
    echo "This ensures users can verify the authenticity of your packages."
    echo ""

    read -p "Do you want to use GPG signing? [Y/n]: " use_gpg

    if [[ $use_gpg =~ ^[Nn]$ ]]; then
        echo "⚠️   Proceeding without GPG signing"
        GPG_SIGN=""
    else
        select_gpg_key
    fi
else
    echo "⚠️   GPG signing disabled by --no-gpg flag"
    GPG_SIGN=""
fi

echo ""

# Always use local .deb file - build if needed
echo "🔍  Ensuring local .deb file exists..."
MANIFEST_FILE="flatpak/net.unstoppableswap.gui.json"
TEMP_MANIFEST=""

# Look for the .deb file in the expected location
DEB_FILE=$(find ./target/release/bundle/deb/ -name "*.deb" -not -name "*.deb.sig" 2>/dev/null | head -1)

if [ -n "$DEB_FILE" ] && [ -f "$DEB_FILE" ]; then
    echo "✅  Found local .deb file: $DEB_FILE"
else
    echo "🏗️   No local .deb file found, building locally..."

    if [ ! -f "./release-build.sh" ]; then
        echo "❌  release-build.sh not found"
        exit 1
    fi

    # Extract version from Cargo.toml
    VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/.*= "//' | sed 's/".*//')
    if [ -z "$VERSION" ]; then
        echo "❌  Could not determine version from Cargo.toml"
        exit 1
    fi

    echo "📦  Building version $VERSION..."
    ./release-build.sh "$VERSION"

    # Look for the .deb file again
    DEB_FILE=$(find ./target/release/bundle/deb/ -name "*.deb" -not -name "*.deb.sig" 2>/dev/null | head -1)
    if [ -z "$DEB_FILE" ] || [ ! -f "$DEB_FILE" ]; then
        echo "❌  Failed to build .deb file"
        exit 1
    fi

    echo "✅  Local build completed: $DEB_FILE"
fi

# Get the absolute path
DEB_ABSOLUTE_PATH=$(realpath "$DEB_FILE")

# Calculate SHA256 hash of the .deb file
echo "🔢  Calculating SHA256 hash..."
DEB_SHA256=$(sha256sum "$DEB_ABSOLUTE_PATH" | cut -d' ' -f1)
echo "   Hash: $DEB_SHA256"

# Create a temporary manifest with the local file
TEMP_MANIFEST=$(mktemp --suffix=.json)

echo "📝  Creating manifest with local .deb..."

# Modify the manifest to use the local file
jq --arg deb_path "file://$DEB_ABSOLUTE_PATH" --arg deb_hash "$DEB_SHA256" '
    .modules[0].sources = [
        {
            "type": "file",
            "url": $deb_path,
            "sha256": $deb_hash,
            "dest": ".",
            "dest-filename": "unstoppableswap.deb"
        }
    ] |
    .modules[0]."build-commands" = [
        "ar -x unstoppableswap.deb",
        "tar -xf data.tar.gz",
        "install -Dm755 usr/bin/unstoppableswap-gui-rs /app/bin/unstoppableswap-gui-rs"
    ]
' "$MANIFEST_FILE" > "$TEMP_MANIFEST"

MANIFEST_FILE="$TEMP_MANIFEST"
echo "📦  Using local build: $(basename "$DEB_FILE")"

echo ""

# Create build directory
rm -rf "$REPO_DIR"
mkdir -p "$REPO_DIR"

# Build arguments
BUILD_ARGS=(
    "build-dir"
    "--user"
    "--install-deps-from=flathub"
    "--disable-rofiles-fuse"
    "--disable-updates"
    "--force-clean"
    "--repo=$REPO_DIR"
)

if [ -n "$GPG_SIGN" ]; then
    BUILD_ARGS+=("--gpg-sign=$GPG_SIGN")
    echo "🔐  GPG signing enabled with key: $GPG_SIGN"
fi

# Add Flathub repository for dependencies
echo "📦  Setting up Flathub repository..."
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo

# Build the Flatpak
echo "🔨  Building Flatpak..."
flatpak-builder "${BUILD_ARGS[@]}" "$MANIFEST_FILE"

# Generate static deltas for faster downloads
echo "⚡  Generating static deltas..."
DELTA_ARGS=("--generate-static-deltas" "--prune")
if [ -n "$GPG_SIGN" ]; then
    DELTA_ARGS+=("--gpg-sign=$GPG_SIGN")
fi
flatpak build-update-repo "${DELTA_ARGS[@]}" "$REPO_DIR"

# Create bundle for direct download
echo "📦  Creating Flatpak bundle..."
BUNDLE_ARGS=("$REPO_DIR" "net.unstoppableswap.gui.flatpak")
if [ -n "$GPG_SIGN" ]; then
    BUNDLE_ARGS+=("--gpg-sign=$GPG_SIGN")
fi
flatpak build-bundle "${BUNDLE_ARGS[@]}" net.unstoppableswap.gui

# Generate .flatpakrepo file
echo "📝  Generating .flatpakrepo file..."
cat > "$REPO_DIR/unstoppableswap.flatpakrepo" << EOF
[Flatpak Repo]
Title=UnstoppableSwap
Name=UnstoppableSwap
Url=${PAGES_URL}/
Homepage=https://github.com/${GITHUB_USER}/${REPO_NAME}
Comment=Unstoppable cross-chain atomic swaps
Description=Repository for UnstoppableSwap applications - providing secure and decentralized XMR-BTC atomic swaps
Icon=${PAGES_URL}/icon.png
SuggestRemoteName=unstoppableswap
EOF

# Add GPG key if signing
if [ -n "$GPG_SIGN" ]; then
    echo "🔑  Adding GPG key to .flatpakrepo..."
    GPG_KEY_B64=$(gpg --export "$GPG_SIGN" | base64 -w 0)
    echo "GPGKey=$GPG_KEY_B64" >> "$REPO_DIR/unstoppableswap.flatpakrepo"
fi

# Generate .flatpakref file
echo "📝  Generating .flatpakref file..."
cat > "$REPO_DIR/net.unstoppableswap.gui.flatpakref" << EOF
[Flatpak Ref]
Title=UnstoppableSwap GUI
Name=net.unstoppableswap.gui
Branch=stable
Url=${PAGES_URL}/
SuggestRemoteName=unstoppableswap
Homepage=https://github.com/${GITHUB_USER}/${REPO_NAME}
Icon=${PAGES_URL}/icon.png
RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo
IsRuntime=false
EOF

# Add GPG key if signing
if [ -n "$GPG_SIGN" ]; then
    GPG_KEY_B64=$(gpg --export "$GPG_SIGN" | base64 -w 0)
    echo "GPGKey=$GPG_KEY_B64" >> "$REPO_DIR/net.unstoppableswap.gui.flatpakref"
fi

# Copy bundle to repo directory
cp net.unstoppableswap.gui.flatpak "$REPO_DIR/"

# Use index.html from flatpak directory
if [ -f "flatpak/index.html" ]; then
    echo "Copying index.html from flatpak directory..."
    cp flatpak/index.html "$REPO_DIR/index.html"
else
    echo "Error: flatpak/index.html not found"
    exit 1
fi

# Copy any additional files
if [ -f "icon.png" ]; then
    cp icon.png "$REPO_DIR/"
fi

if [ -f "README.md" ]; then
    cp README.md "$REPO_DIR/"
fi

# Add .nojekyll file to skip Jekyll processing
touch "$REPO_DIR/.nojekyll"

echo "✅  Flatpak repository built successfully!"
echo "📊  Repository size: $(du -sh $REPO_DIR | cut -f1)"
echo "📁  Repository files are in: $REPO_DIR/"

if [ "$PUSH_FLAG" = "--push" ]; then
    echo ""
    echo "🚀  Deploying to GitHub Pages..."

    # Store current branch
    CURRENT_BRANCH=$(git branch --show-current)

    # Create a temporary directory for deployment
    DEPLOY_DIR=$(mktemp -d)

    # Copy flatpak repo to deploy directory (including hidden files)
    echo "📁  Preparing deployment files..."
    cp -r "$REPO_DIR"/. "$DEPLOY_DIR/"

    # Initialize fresh git repo in deploy directory
    cd "$DEPLOY_DIR"
    git init
    git add .
    git commit -m "Update Flatpak repository $(date -u '+%Y-%m-%d %H:%M:%S UTC')"

    # Go back to original directory
    cd - > /dev/null

    # Push to GitHub Pages branch
    echo "🚀  Force pushing to $BRANCH..."
    cd "$DEPLOY_DIR"
    git remote add origin "$(cd - > /dev/null && git remote get-url origin)"
    git push --force origin HEAD:"$BRANCH"

    # Return to original directory and clean up
    cd - > /dev/null
    rm -rf "$DEPLOY_DIR"

    echo "🎉  Deployed successfully!"
    echo "🌐  Your Flatpak repository is available at: $PAGES_URL"
    echo ""
    echo "📋  Users can install with:"
    echo "   flatpak remote-add --user unstoppableswap $PAGES_URL/unstoppableswap.flatpakrepo"
    echo "   flatpak install unstoppableswap net.unstoppableswap.gui"
    echo ""
    if [ -n "$GPG_SIGN" ]; then
        echo "🔐  Repository is signed with GPG key: $GPG_SIGN"
    fi
else
    echo ""
    echo "📋  To deploy to GitHub Pages, run:"
    echo "   $0 --push"
    echo ""
    echo "📋  Or manually copy the contents of $REPO_DIR/ to your gh-pages branch"
fi

# Cleanup temporary manifest if created
if [ -n "$TEMP_MANIFEST" ] && [ -f "$TEMP_MANIFEST" ]; then
    rm -f "$TEMP_MANIFEST"
fi

# Cleanup
rm -rf "$TEMP_DIR"
echo "🧹  Cleanup completed"