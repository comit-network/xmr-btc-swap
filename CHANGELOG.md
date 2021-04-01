# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A changelog file.

### Fixed

- An [issue](https://github.com/comit-network/xmr-btc-swap/issues/353) where the `swap` CLI would fail on systems that were set to a locale different than English.
  A bad readiness check when waiting for `monero-wallet-rpc` to be ready caused the CLI to hang forever, preventing users from perform a swap.

### Security

- Fixed an issue where Alice would not verify if Bob's Bitcoin lock transaction is semantically correct, i.e. pays the agreed upon amount to an output owned by both of them.
  Fixing this required a **breaking change** on the network layer and hence old versions are not compatible with this version.

[Unreleased]: https://github.com/comit-network/xmr-btc-swap/compare/v0.3...HEAD
