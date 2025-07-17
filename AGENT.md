# Repo Overview

This repository hosts the core of the eigenwallet project. The code base is a Rust workspace with multiple crates and a Tauri based GUI.

## Important directories

- **swap/** – contains the main Rust crate with two binaries:
  - `swap` – command line interface for performing swaps.
  - `asb` – Automated Swap Backend for market makers.
    It also hosts library code shared between the binaries and integration tests.
- **src-tauri/** – Rust crate that exposes the `swap` functionality to the Tauri front end and bundles the application.
- **src-gui/** – The front‑end written in TypeScript/React and bundled by Tauri. Communicates with `src-tauri` via Tauri commands.
- **monero-rpc/** and **monero-wallet/** – helper crates for interacting with the Monero ecosystem.
- **docs/** – Next.js documentation site.
- **dev-docs/** – additional markdown documentation for CLI and ASB.

## Frequently edited files

Looking at the latest ten pull requests in `git log`, the following files appear most often:

| File                        | Times Changed |
| --------------------------- | ------------- |
| `src-tauri/Cargo.toml`      | 7             |
| `Cargo.lock`                | 7             |
| `CHANGELOG.md`              | 7             |
| `swap/Cargo.toml`           | 6             |
| `src-tauri/tauri.conf.json` | 5             |
| `.github/workflows/ci.yml`  | 3             |

Other files such as `swap/src/bin/asb.rs`, `swap/src/cli/api.rs`, and `src-gui/package.json` showed up less frequently.

## Component interaction

- The **swap** crate implements the atomic swap logic and provides a CLI. The binaries under `swap/src/bin` (`swap.rs` and `asb.rs`) start the client and maker services respectively.
- **src-tauri** wraps the swap crate and exposes its functionality to the GUI via Tauri commands. It also bundles the application with the `src-gui` assets.
- **src-gui** is the TypeScript/React interface. It communicates with the Rust back end through the commands defined in `src-tauri`.
- Helper crates like **monero-rpc** and **monero-wallet** provide abstractions over external services. They are used by the swap crate to interact with Monero.
- Continuous integration and release workflows live in `.github/workflows`. They build binaries, create releases and lint the code base.

## Pull request titles

Use descriptive titles following the `<type>(<scope>): <description>` format. Examples include:

- `feat(gui): New feature`
- `fix(swap): Issue fixed`
- `refactor(ci): Ci changes`
