# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Minimum Supported Rust Version (MSRV) bumped to 1.70

## [0.12.3] - 2023-09-20

- Swap: If no Monero daemon is manually specified, we will automatically choose one from a list of public daemons by connecting to each and checking their availability.

## [0.12.2] - 2023-08-08

### Changed

- Minimum Supported Rust Version (MSRV) bumped to 1.67
- ASB can now register with multiple rendezvous nodes. The `rendezvous_point` option in `config.toml` can be a string with comma separated addresses, or a toml array of address strings.

## [0.12.1] - 2023-01-09

### Changed

- Swap: merge separate cancel/refund commands into one `cancel-and-refund` command for stuck swaps

## [0.12.0] - 2022-12-31

### Changed

- Update `bdk` library to latest version. This introduces an incompatability with previous versions due to different formats being used to exchange Bitcoin transactions
- Changed ASB to quote on Monero unlocked balance instead of total balance
- Allow `asb` to set a bitcoin address that is controlled by the asb itself to redeem/punish bitcoin to

### Added

- Allow asb config overrides using environment variables. See [1231](https://github.com/comit-network/xmr-btc-swap/pull/1231)

## [0.11.0] - 2022-08-11

### Changed

- Update from Monero v0.17.2.0 to Monero v0.18.0.0
- Change Monero nodes to [Rino tool nodes](https://community.rino.io/nodes.html)
- Always write logs as JSON to files
- Change to UTC time for log messages, due to a bug causing no logging at all to be printed (linux/macos), and an [unsoundness issue](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/time/struct.LocalTime.html) with local time in [the time crate](https://github.com/time-rs/time/issues/293#issuecomment-748151025)
- Fix potential integer overflow in ASB when calculating maximum Bitcoin amount for Monero balance
- Reduce Monero locking transaction fee amount from 0.000030 to 0.000016 XMR, which is still double the current median fee as reported at [monero.how](https://www.monero.how/monero-transaction-fees)

### Added

- Adjust quote based on Bitcoin balance.
  If the max_buy_btc in the ASB config is higher than the available balance to trade, it will return the max available balance discounting the Monero locking fees. In the case the balance is lower than the min_buy_btc config it will return 0 to the CLI. If the ASB returns a quote of 0 the CLI will not allow you continue with a trade.
- Reduce required confirmations for Bitcoin transactions from 2 to 1
- Both the ASB and CLI now support the [Identify](https://github.com/libp2p/specs/blob/master/identify/README.md) protocol. This makes its version and network (testnet/mainnet) avaliable to others
- Display minimum BTC deposit required to cover the minimum quantity plus fee in the Swap CLI
- Swap CLI will check its monero-wallet-rpc version and remove it if it's older than Fluorine Fermi (0.18)

## [0.10.2] - 2021-12-25

### Changed

- Record monero wallet restore blockheight in state `SwapSetupCompleted` already.
  This solves issues where the CLI went offline after sending the BTC transaction, and the monero wallet restore blockheight being recorded after Alice locked the Monero, resulting in the generated XMR redeem wallet not detecting the transaction and reporting `No unlocked balance in the specified account`.
  This is a breaking database change!
  Swaps that were saved prior to this change may fail to load if they are in state `SwapSetupCompleted` of `BtcLocked`.
  Make sure to finish your swaps before upgrading.

## [0.10.1] - 2021-12-23

### Added

- `monero-recovery` command that can be used to print the monero address, private spend and view key so one can manually recover instances where the `monero-wallet-rpc` does not pick up the Monero funds locked up by the ASB.
  Related issue: <https://github.com/comit-network/xmr-btc-swap/issues/537>
  The command takes the swap-id as parameter.
  The swap has to be in a `BtcRedeemed` state.
  Use `--help` for more details.

## [0.10.0] - 2021-10-15

### Removed

- Support for the old sled database.
  The ASB and CLI only support the new sqlite database.
  If you haven't already, you can migrate your old data using the 0.9.0 release.

### Changed

- The ASB to no longer work as a rendezvous server.
  The ASB can still register with rendezvous server as usual.

### Fixed

- Mitigate CloseNotify bug #797 by retrying getting ScriptStatus if it fail and using a more stable public mainnet electrum server.

## [0.9.0] - 2021-10-07

### Changed

- Timestamping is now enabled by default even when the ASB is not run inside an interactive terminal.
- The `cancel`, `refund` and `punish` subcommands in ASB and CLI are run with the `--force` by default and the `--force` option has been removed.
  The force flag was used to ignore blockheight and protocol state checks.
  Users can still restart a swap with these checks using the `resume` subcommand.
- Changed log level of the "Advancing state", "Establishing Connection through Tor proxy" and "Connection through Tor established" log message from tracing to debug in the CLI.
- ASB and CLI can migrate their data to sqlite to store swaps and related data.
  This makes it easier to build applications on top of xmr-btc-swap by enabling developers to read swap information directly from the database.
  This resolved an issue where users where unable to run concurrent processes, for example, users could not print the swap history if another ASB or CLI process was running.
  The sqlite database filed is named `sqlite` and is found in the data directory.
  You can print the data directory using the `config` subcommand.
  The schema can be found here [here](swap/migrations/20210903050345_create_swaps_table.sql).

#### Database migration guide

##### Delete old data

The simplest way to migrate is to accept the loss of data and delete the old database.

1. Find the location of the old database using the `config` subcommand.
2. Delete the database
3. Run xmr-btc-swap
   xmr-btc swap will create a new sqlite database and use that from now on.

##### Preserve old data

It is possible to migrate critical data from the old db to the sqlite but there are many pitfalls.

1. Run xmr-btc-swap as you would normally
   xmr-btc-swap will try and automatically migrate your existing data to the new database.
   If the existing database contains swaps for very early releases, the migration will fail due to an incompatible schema.
2. Print out the swap history using the `history` subcommand.
3. Print out the swap history stored in the old database by also passing the `--sled` flag.
   eg. `swap-cli --sled history`
4. Compare the old and new history to see if you are happy with migration.
5. If you are unhappy with the new history you can continue to use the old database by passing the `--sled flag`

### Added

- Added a `disable-timestamp` flag to the ASB that disables timestamps from logs.
- A `config` subcommand that prints the current configuration including the data directory location.
  This feature should alleviate difficulties users were having when finding where xmr-btc-swap was storing data.
- Added `export-bitcoin-wallet` subcommand to the CLI and ASB, to print the internal bitcoin wallet descriptor.
  This will allow users to transact and monitor using external wallets.

### Removed

- The `bitcoin-target-block` argument from the `balance` subcommand on the CLI.
  This argument did not affect how the balance was calculated and was pointless.

## [0.8.3] - 2021-09-03

### Fixed

- A bug where the ASB erroneously transitioned into a punishable state upon a bitcoin transaction monitoring error.
  This could lead to a scenario where the ASB was neither able to punish, nor able to refund, so the XMR could stay locked up forever while the CLI refunded the BTC.
- A bug where the CLI erroneously transitioned into a cancel-timelock-expired state upon a bitcoin transaction monitoring error.
  This could lead to a scenario where the CLI is forced to wait for cancel, even though the cancel timelock is not yet expired and the swap could still be redeemed.

## [0.8.2] - 2021-09-01

### Added

- Add the ability to view the swap-cli bitcoin balance and withdraw.
  See issue <https://github.com/comit-network/xmr-btc-swap/issues/694>.

### Fixed

- An issue where the connection between ASB and CLI would get closed prematurely.
  The CLI expects to be connected to the ASB throughout the entire swap and hence reconnects as soon as the connection is closed.
  This resulted in a loop of connections being established but instantly closed again because the ASB deemed the connection to not be necessary.
  See issue <https://github.com/comit-network/xmr-btc-swap/issues/648>.
- An issue where the ASB was unable to use the Monero wallet in case `monero-wallet-rpc` has been restarted.
  In case no wallet is loaded when we try to interact with the `monero-wallet-rpc` daemon, we now load the correct wallet on-demand.
  See issue <https://github.com/comit-network/xmr-btc-swap/issues/652>.
- An issue where swap protocol was getting stuck trying to submit the cancel transaction.
  We were not handling the error when TxCancel submission fails.
  We also configured the electrum client to retry 5 times in order to help with this problem.
  See issues: <https://github.com/comit-network/xmr-btc-swap/issues/709> <https://github.com/comit-network/xmr-btc-swap/issues/688>, <https://github.com/comit-network/xmr-btc-swap/issues/701>.
- An issue where the ASB withdraw one bitcoin UTXO at a time instead of the whole balance.
  See issue <https://github.com/comit-network/xmr-btc-swap/issues/662>.

## [0.8.1] - 2021-08-16

### Fixed

- An occasional error where users couldn't start a swap because of `InsufficientFunds` that were off by exactly 1 satoshi.

## [0.8.0] - 2021-07-09

### Added

- Printing the deposit address to the terminal as a QR code.
  To not break automated scripts or integrations with other software, this behaviour is disabled if `--json` is passed to the application.
- Configuration setting for the websocket URL that the ASB connects to in order to receive price ticker updates.
  Can be configured manually by editing the config.toml file directly.
  It is expected that the server behind the url follows the same protocol as the [Kraken websocket api](https://docs.kraken.com/websockets/).
- Registration and discovery of ASBs using the [libp2p rendezvous protocol](https://github.com/libp2p/specs/blob/master/rendezvous/README.md).
  ASBs can register with a rendezvous node upon startup and, once registered, can be automatically discovered by the CLI using the `list-sellers` command.
  The rendezvous node address (`rendezvous_point`), as well as the ASB's external addresses (`external_addresses`) to be registered, is configured in the `network` section of the ASB config file.
  A rendezvous node is provided at `/dnsaddr/rendezvous.coblox.tech/p2p/12D3KooWQUt9DkNZxEn2R5ymJzWj15MpG6mTW84kyd8vDaRZi46o` for testing purposes.
  Upon discovery using `list-sellers` CLI users are provided with quote and connection information for each ASB discovered through the rendezvous node.
- A mandatory `--change-address` parameter to the CLI's `buy-xmr` command.
  The provided address is used to transfer Bitcoin in case of a refund and in case the user transfers more than the specified amount into the swap.
  For more information see [#513](https://github.com/comit-network/xmr-btc-swap/issues/513).

### Fixed

- An issue where the ASB gives long price guarantees when setting up a swap.
  Now, after sending a spot price the ASB will wait for one minute for the CLI's to trigger the execution setup, and three minutes to see the BTC lock transaction of the CLI in mempool after the swap started.
  If the first timeout is triggered the execution setup will be aborted, if the second timeout is triggered the swap will be safely aborted.
- An issue where the default Monero node connection string would not work, because the public nodes were moved to a different domain.
  The default monerod nodes were updated to use the [melo tool nodes](https://melo.tools/nodes.html).

### Changed

- The commandline interface of the CLI to combine `--seller-addr` and `--seller-peer-id`.
  These two parameters have been merged into a parameter `--seller` that accepts a single [multiaddress](https://docs.libp2p.io/concepts/addressing/).
  The multiaddress must end with a `/p2p` protocol defining the seller's peer ID.
- The `--data-dir` option to `--data-base-dir`.
  Previously, this option determined the final data directory, regardless of the `--testnet` flag.
  With `--data-base-dir`, a subdirectory (either `testnet` or `mainnet`) will be created under the given path.
  This allows using the same command with or without `--testnet`.

### Removed

- The websocket transport from the CLI.
  Websockets were only ever intended to be used for the ASB side to allow websites to retrieve quotes.
  The CLI can use regular TCP connections and having both - TCP and websockets - causes problems and unnecessary overhead.
- The `--seller-addr` parameter from the CLI's `resume` command.
  This information is now loaded from the database.
- The `--receive-address` parameter from the CLI's `resume` command.
  This information is now loaded from the database.

## [0.7.0] - 2021-05-28

### Fixed

- An issue where long-running connections are dead without a connection closure being reported back to the swarm.
  Adding a periodic ping ensures that the connection is kept alive, and a broken connection is reported back resulting in a close event on the swarm.
  This fixes the error of the ASB being unable to send a transfer proof to the CLI.
- An issue where ASB Bitcoin withdrawal can be done to an address on the wrong network.
  A network check was added that compares the wallet's network against the network of the given address when building the transaction.

## [0.6.0] - 2021-05-24

### Added

- Cancel command for the ASB that allows cancelling a specific swap by id.
  Using the cancel command requires the cancel timelock to be expired, but `--force` can be used to circumvent this check.
- Refund command for the ASB that allows refunding a specific swap by id.
  Using the refund command to refund the XMR locked by the ASB requires the CLI to first refund the BTC of the swap.
  If the BTC was not refunded yet the command will print an error accordingly.
  The command has a `--force` flag that allows executing the command without checking for cancel constraints.
- Punish command for the ASB that allows punishing a specific swap by id.
  Includes a `--force` parameter that when set disables the punish timelock check and verifying that the swap is in a cancelled state already.
- Abort command for the ASB that allows safely aborting a specific swap.
  Only swaps in a state prior to locking XMR can be safely aborted.
- Redeem command for the ASB that allows redeeming a specific swap.
  Only swaps where we learned the encrypted signature are redeemable.
  The command checks for expired timelocks to ensure redeeming is safe, but the timelock check can be disable using the `--force` flag.
  By default we wait for finality of the redeem transaction; this can be disabled by setting `--do-not-await-finality`.
- Resume-only mode for the ASB.
  When started with `--resume-only` the ASB does not accept new, incoming swap requests but only finishes swaps that are resumed upon startup.
- A minimum accepted Bitcoin amount for the ASB similar to the maximum amount already present.
  For the CLI the minimum amount is enforced by waiting until at least the minimum is available as max-giveable amount.
- Added a new argument to ASB: `--json` or `-j`. If set, log messages will be printed in JSON format.

### Fixed

- An issue where both the ASB and the CLI point to the same default directory `xmr-btc-swap` for storing data.
  The asb now uses `xmr-btc-swap/asb` and the CLI `xmr-btc-swap/cli` as default directory.
  This is a breaking change.
  If you want to access data created by a previous version you will have to rename the data folder or one of the following:
  1. For the CLI you can use `--data-dir` to point to the old directory.
  2. For the ASB you can change the data-dir in the config file of the ASB.
- The CLI receives proper Error messages if setting up a swap with the ASB fails.
  This is a breaking change because the spot-price protocol response changed.
  Expected errors scenarios that are now reported back to the CLI:
  1. Balance of ASB too low
  2. Buy amount sent by CLI exceeds maximum buy amount accepted by ASB
  3. ASB is running in resume-only mode and does not accept incoming swap requests
- An issue where the monero daemon port used by the `monero-wallet-rpc` could not be specified.
  The CLI parameter `--monero-daemon-host` was changed to `--monero-daemon-address` where host and port have to be specified.
- An issue where an ASB redeem scenario can transition to a cancel and publish scenario that will fail.
  This is a breaking change for the ASB, because it introduces a new state into the database.

### Changed

- The ASB's `--max-buy` and `ask-spread` parameter were removed in favour of entries in the config file.
  The initial setup includes setting these two values now.
- From this version on the CLI and ASB run on **mainnet** by default!
  When running either application with `--testnet` Monero network defaults to `stagenet` and Bitcoin network to `testnet3`.
  This is a breaking change.
  It is recommended to run the applications with `--testnet` first and not just run the application on `mainnet` without experience.

## [0.5.0] - 2021-04-17

### Changed

- The quote protocol returns JSON encoded data instead of CBOR.
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
- Tor support as an optional feature.
  If ASB detects that Tor's control port is open, a hidden service is created for
  the network it is listening on (currently 2).
  The Tor control port as well as Tor socks5 proxy port is configurable.

## [0.4.0] - 2021-04-06

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
- The `resume` command of the `swap` CLI no longer require the `--seller-peer-id` parameter.
  This information is now saved in the database.

### Fixed

- An [issue](https://github.com/comit-network/xmr-btc-swap/issues/353) where the `swap` CLI would fail on systems that were set to a locale different than English.
  A bad readiness check when waiting for `monero-wallet-rpc` to be ready caused the CLI to hang forever, preventing users from perform a swap.

### Security

- Fixed an issue where Alice would not verify if Bob's Bitcoin lock transaction is semantically correct, i.e. pays the agreed upon amount to an output owned by both of them.
  Fixing this required a **breaking change** on the network layer and hence old versions are not compatible with this version.

[Unreleased]: https://github.com/comit-network/xmr-btc-swap/compare/0.12.3...HEAD
[0.12.3]: https://github.com/comit-network/xmr-btc-swap/compare/0.12.2...0.12.3
[0.12.2]: https://github.com/comit-network/xmr-btc-swap/compare/0.12.1...0.12.2
[0.12.1]: https://github.com/comit-network/xmr-btc-swap/compare/0.12.0...0.12.1
[0.12.0]: https://github.com/comit-network/xmr-btc-swap/compare/0.11.0...0.12.0
[0.11.0]: https://github.com/comit-network/xmr-btc-swap/compare/0.10.2...0.11.0
[0.10.2]: https://github.com/comit-network/xmr-btc-swap/compare/0.10.1...0.10.2
[0.10.1]: https://github.com/comit-network/xmr-btc-swap/compare/0.10.0...0.10.1
[0.10.0]: https://github.com/comit-network/xmr-btc-swap/compare/0.9.0...0.10.0
[0.9.0]: https://github.com/comit-network/xmr-btc-swap/compare/0.8.3...0.9.0
[0.8.3]: https://github.com/comit-network/xmr-btc-swap/compare/0.8.2...0.8.3
[0.8.2]: https://github.com/comit-network/xmr-btc-swap/compare/0.8.1...0.8.2
[0.8.1]: https://github.com/comit-network/xmr-btc-swap/compare/0.8.0...0.8.1
[0.8.0]: https://github.com/comit-network/xmr-btc-swap/compare/0.7.0...0.8.0
[0.7.0]: https://github.com/comit-network/xmr-btc-swap/compare/0.6.0...0.7.0
[0.6.0]: https://github.com/comit-network/xmr-btc-swap/compare/0.5.0...0.6.0
[0.5.0]: https://github.com/comit-network/xmr-btc-swap/compare/0.4.0...0.5.0
[0.4.0]: https://github.com/comit-network/xmr-btc-swap/compare/v0.3...0.4.0
