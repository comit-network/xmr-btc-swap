# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Quote protocol returns JSON encoded data instead of CBOR.
  This is a breaking change in the protocol handling, old CLI versions will not be able to process quote requests of ASBs running this version.

### Fixed

- An issue where concurrent swaps with the same peer would cause the ASB to handle network communication incorrectly.
  To fix this, all messages are now tagged with a unique identifier that is agreed upon at the start of the swap.
  This is a breaking change in the network layer and hence old versions are not compatible with this version.
  We advise to also not resume any swaps that have been created with an older version.
  It is recommended to reset / delete the database after upgrading.
- An issue where the CLI would not reconnect to the ASB in case the network connection dropped.
  We now attempt to re-establish the connection using an exponential backoff but will give up eventually after 5 minutes.

### Added

- Websocket support for the ASB.
  The ASB is now capable to listen on both TCP and Websocket connections.
  Default websocket listening port is 9940.

## [0.4.0] - 2021-04-06

### Changed

- The `resume` command of the `swap` CLI no longer require the `--seller-peer-id` parameter.
  This information is now saved in the database.

### Added

- A changelog file.
- Automatic resume of unfinished swaps for the `asb` upon startup.
  Unfinished swaps from earlier versions will be skipped.
- A configurable spread for the ASB that is applied to the asking price received from the Kraken price ticker.
  The default value is 2% and can be configured using the `--ask-spread` parameter.
  See `./asb --help` for details.

### Changed

- Require the buyer to specify the connection details of the peer they wish to swap with.
  Throughout the public demo phase of this project, the CLI traded with us by default if the peer id and multiaddress of the seller were not specified.
  Having the defaults made it easy for us to give something to the community that can easily be tested, however it is not aligned with our long-term vision of a decentralised network of sellers.
  We have removed these defaults forcing the user to specify the seller they wish to trade with.

### Fixed

- An [issue](https://github.com/comit-network/xmr-btc-swap/issues/353) where the `swap` CLI would fail on systems that were set to a locale different than English.
  A bad readiness check when waiting for `monero-wallet-rpc` to be ready caused the CLI to hang forever, preventing users from perform a swap.

### Security

- Fixed an issue where Alice would not verify if Bob's Bitcoin lock transaction is semantically correct, i.e. pays the agreed upon amount to an output owned by both of them.
  Fixing this required a **breaking change** on the network layer and hence old versions are not compatible with this version.

[Unreleased]: https://github.com/comit-network/xmr-btc-swap/compare/0.4.0...HEAD
[0.4.0]: https://github.com/comit-network/xmr-btc-swap/compare/v0.3...0.4.0
