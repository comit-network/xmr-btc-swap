# Show help for each of the justfile recipes
help:
	@just --list

# Build Monero C++ Codebase (currently disabled)
# build_monero_cpp:
#	just update_submodules
#	cd monero-sys/monero && make -j8 release

# Clean the Monero C++ Codebase
clean_monero_cpp:
	rm -rf monero-sys/monero/
	just update_submodules

# Builds the Rust bindings for Monero
monero_sys:
	just update_submodules
	cd monero-sys && cargo build

# Test the FFI bindings using various sanitizers, that can detect memory safety issues.
test-ffi: test-ffi-address

# Tests the FFI bindings using AddressSanitizer (https://doc.rust-lang.org/beta/unstable-book/compiler-flags/sanitizer.html#addresssanitizer). Can detect memory safety issues like use-after-free, double-free, leaks, etc.
test-ffi-address:
	cd monero-sys && RUSTFLAGS=-Zsanitizer=address cargo +nightly nextest run -Zbuild-std --target=`rustc --version --verbose | grep "host:" | cut -d' ' -f2`

# Start the Tauri app
tauri:
	cd src-tauri && cargo tauri dev --no-watch --verbose -- -- --testnet

tauri-mainnet:
	cd src-tauri && cargo tauri dev --no-watch

# Install the GUI dependencies
gui_install:
	cd src-gui && yarn install

# Start the GUI Dev Server
web:
	cd src-gui && yarn dev

gui:
	just web & just tauri

gui-mainnet:
	just web & just tauri-mainnet

# Build the GUI
gui_build:
        cd src-gui && yarn build

# Run the Rust tests
tests:
        cargo nextest run

# Tests the Rust bindings for Monero
test_monero_sys:
        cd monero-sys && cargo nextest run

# Builds the ASB and Swap binaries
swap:
	cd swap && cargo build --bin asb --bin=swap

# Run the asb on testnet
asb-testnet:
	cd swap && cargo run --bin asb -- --trace --testnet start

# Updates our submodules (currently only Monero C++ codebase)
update_submodules:
	cd monero-sys && git submodule update --init --recursive --force

# Run clippy checks
clippy:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

# Generate the bindings for the Tauri API
bindings:
	cd src-gui && yarn run gen-bindings

# Format the code
fmt:
	dprint fmt

# Run eslint for the GUI frontend
check_gui_eslint:
	cd src-gui && yarn run eslint

# Run the typescript type checker for the GUI frontend
check_gui_tsc:
	cd src-gui && yarn run tsc --noEmit

# Run the checks for the GUI frontend
check_gui:
	just check_gui_eslint || true
	just check_gui_tsc

# Sometimes you have to prune the docker network to get the integration tests to work
docker-prune-network:
	docker network prune -f

# Install dependencies required for building monero-sys
prepare_mac_os_brew_dependencies:
	cd dev_scripts && chmod +x ./brew_dependencies_install.sh && ./brew_dependencies_install.sh

# Takes a crate (e.g monero-rpc-pool) and uses code2prompt to copy to clipboard
# E.g code2prompt . --exclude "*.lock" --exclude ".sqlx/*" --exclude "target"
code2prompt_single_crate crate:
	cd {{crate}} && code2prompt . --exclude "*.lock" --exclude ".sqlx/*" --exclude "target"
