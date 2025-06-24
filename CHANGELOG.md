# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- ASB + GUI + CLI: We now cache fee estimates for the Bitcoin wallet for up to 2 minutes. This improves the speed of fee estimation and reduces the number of requests to the Electrum servers.

## [2.3.0-beta.1] - 2025-06-19

- ASB + CLI + GUI: Introduce a load-balancing proxy for Monero RPC nodes that automatically discovers healthy nodes and routes requests to improve connection reliability.
- ASB: Added `monero_node_pool` boolean option to ASB config. When enabled, the ASB uses the internal Monero RPC pool instead of connecting directly to a single daemon URL, providing improved reliability and automatic failover across multiple Monero nodes.

## [2.2.0-beta.2] - 2025-06-17

- We now call Monero function directly (via FFI bindings) instead of using `monero-wallet-rpc`.
- ASB: Since we don't communicate with `monero-wallet-rpc` anymore, the Monero wallet's will no longer be accessible by connecting to it. If you are using the asb-docker-compose setup, run this command to migrate the wallet files from the volume of the monero-wallet-rpc container to the volume of the asb container:
  ```bash
  # On testnet
  cp /var/lib/docker/volumes/testnet_stagenet_monero-wallet-rpc-data/_data/* /var/lib/docker/volumes/testnet_testnet_asb-data/_data/monero/wallets
  # On mainnet
  cp /var/lib/docker/volumes/mainnet_mainnet_monero-wallet-rpc-data/_data/* /var/lib/docker/volumes/mainnet_mainnet_asb-data/_data/monero/wallets
  ```
- ASB: The `wallet_url` option has been removed and replaced with the optional `daemon_url`, that specifies which Monero node the asb will connect to. If not specified, the asb will connect to a known public Monero node at random.
- ASB: Add a `export-monero-wallet` command which gives the Monero wallet's seed and restore height. Export this seed into a wallet software of your own choosing to manage your Monero funds.
  The seed is a 25 word mnemonic. Example:
  ```bash
  $ asb export-monero-wallet > wallet.txt
  $ cat wallet.txt
  Seed          : novelty deodorant aloof serving fuel vipers awful segments siblings bite exquisite quick snout rising hobby trash amply recipe cinema ritual problems pram getting playful novelty
  Restore height: 3403755
  $
  ```

- Logs are now written to `stderr` (instead of `stdout`). Makers relying on piping the logs need to make sure to include the `stderr` output:

  | Before                     | After                            |
  | -------------------------- | -------------------------------- |
  | `asb logs \| my-script.sh` | `asb logs  2>&1 \| my-script.sh` |
  | `asb logs > output.txt`    | `asb logs > output.txt 2>&1`     |
- GUI: Improved peer discovery: We can now connect to multiple rendezvous points at once. We also cache peers we have previously connected to locally and will attempt to connect to them again in the future, even if they aren't registered with a rendezvous point anymore.
- ASB: We now retry for 6 hours to broadcast the early refund transaction. After that, we give up and Bob will have to wait for the timelock to expire then refund himself. If we detect that Bob has cancelled the swap, we will abort the swap on our side and let Bob refund himself.

## [2.0.3] - 2025-06-12

## [2.0.2] - 2025-06-12

- GUI: Fix issue where auto updater would not display the update
- ASB + GUI + CLI: Increase request_timeout to 7s, min_retries to 10 for Electrum load balancer

## [2.0.0] - 2025-06-12

- GUI: Build Flatpak bundle in release workflow
- docs: add instructions for verifying Tauri signature files
- docs: document new `electrum_rpc_urls` and `use_mempool_space_fee_estimation` options
- docs: Instructions for verifying GUI (Tauri) signature files

## [2.0.0-beta.2] - 2025-06-11

## [2.0.0-beta.1] - 2025-06-11

- BREAKING PROTOCOL CHANGE: Takers/GUIs running `>= 2.0.0` will not be able to initiate new swaps with makers/asbs running `< 2.0.0`. Please upgrade as soon as possible. Already started swaps from older versions are not be affected.
  - Taker and Maker now collaboratively sign a `tx_refund_early` Bitcoin transaction in the negotiation phase which allows the maker to refund the Bitcoin for the taker without having to wait for the 12h cancel timelock to expire.
  - `tx_refund_early` will only be published if the maker has not locked their Monero yet. This allows swaps to be refunded quickly if the maker doesn't have enough funds available or their daemon is not fully synced. The taker can then use the refunded Bitcoin to start a new swap.
- ASB: The maker will take Monero funds needed for ongoing swaps into consideration when making a quote. A warning will be displayed if the Monero funds do not cover all ongoing swaps.
- ASB: Return a zero quote when quoting fails instead of letting the request time out
- GUI + CLI + ASB: We now do load balancing over multiple Electrum servers. This improves the reliability of all our interactions with the Bitcoin network. When transactions are published they are broadcast to all servers in parallel.
- ASB: The `electrum_rpc_url` option has been removed. A new `electrum_rpc_urls` option has been added. Use it to specify a list of Electrum servers to use. If you want you can continue using a single server by providing a single URL. For most makers we recommend:
  - Running your own [electrs](https://github.com/romanz/electrs/) server
  - Optionally providing 2-5 fallback servers. The order of the servers does matter. Electrum servers at the front of the list have priority and will be tried first. You should place your own server at the front of the list.
  - A list of public Electrum servers can be found [here](https://1209k.com/bitcoin-eye/ele.php?chain=btc)

## [1.1.7] - 2025-06-04

- ASB: Fix an issue where the asb would quote a max_swap_amount off by a couple of piconeros

## [1.1.4] - 2025-06-04

## [1.1.3] - 2025-05-31

- The Bitcoin fee estimation is now more accurate. It uses a combination of `estimatesmartfee` from Bitcoin Core and `mempool.get_fee_histogram` from Electrum to ensure our distance from the mempool tip is appropriate. If our Electrum server doesn't support fee estimation, we use the mempool.space API. The mempool space API can be disabled using the `bitcoin.use_mempool_space_fee_estimation` option in the config file. It defaults to `true`.
- ASB: You can use the `--trace` flag to log all messages to the terminal. This is useful for debugging but shouldn't be used in production because it will log a lot of data, especially related to p2p networking and tor bootstrapping. If you want to debug issues in production, read the tracing-logs inside the data directory instead.

## [1.1.2] - 2025-05-24

- Docs: Document `external_bitcoin_address` option for using a specific
  Bitcoin address when redeeming or punishing swaps.
- Removed the JSON-RPC daemon and the `start-daemon` CLI command.
- Increased the max Bitcoin fee to up to 20% of the value of the transaction. This mitigates an issue where the Bitcoin lock transaction would not get confirmed in time on the blockchain.

## [1.1.1] - 2025-05-20

- CLI + GUI + ASB: Retry the Bitcoin wallet sync up to 15 seconds to work around transient errors.

## [1.1.0] - 2025-05-19

- GUI: Discourage swapping with makers running `< 1.1.0-rc.3` because the bdk upgrade introduced a breaking change.
- GUI: Fix an issue where the auto updater would incorrectly throw an error

## [1.1.0-rc.3] - 2025-05-18

- Breaking Change(Makers): Please complete all pending swaps, then upgrade as soon as possible. Takers might not be able to taker your offers until you upgrade your asb instance.
- CLI + ASB + GUI: We upgraded dependencies related to the Bitcoin wallet. When you boot up the new version for the first time, a migration process will be run to convert the old wallet format to the new one. This might take a few minutes. We also fixed a bug where we would generate too many unused addresses in the Bitcoin wallet which would cause the wallet to take longer to start up as time goes on.
- GUI: We display detailed progress about running background tasks (Tor bootstrapping, Bitcoin wallet sync progress, etc.)

## [1.0.0-rc.21] - 2025-05-15

## [1.0.0-rc.20] - 2025-05-14

- GUI: Added introduction flow for first-time users
- CLI + GUI: Update monero-wallet-rpc to v0.18.4.0

## [1.0.0-rc.19] - 2025-04-28

## [1.0.0-rc.18] - 2025-04-28

- GUI: Feedback submitted can be responded to by the core developers. The responses will be displayed under the "Feedback" tab.

## [1.0.0-rc.17] - 2025-04-18

- GUI: The user will now be asked to approve the swap offer again before the Bitcoin lock transaction is published. Makers should take care to only assume a swap has been accepted by the taker if the Bitcoin lock transaction is detected (`Advancing state state=bitcoin lock transaction in mempool ...`). Swaps that have been safely aborted will not be displayed in the GUI anymore.

## [1.0.0-rc.16] - 2025-04-17

- ASB: Quotes are now cached (Time-to-live of 2 minutes) to avoid overloading the maker with requests in times of high demand

## [1.0.0-rc.14] - 2025-04-16

- CI: Update Rust version to 1.80
- GUI: Update social media links

## [1.0.0-rc.13] - 2025-01-24

- Docs: Added a dedicated page for makers.
- Docs: Improved the refund and punish page.
- ASB: Fixed an issue where the ASB would silently fail if the publication of the Monero refund transaction failed.
- GUI: Add a button to open the data directory for troubleshooting purposes.

## [1.0.0-rc.12] - 2025-01-14

- GUI: Fixed a bug where the CLI wasn't passed the correct Monero node.

## [1.0.0-rc.11] - 2024-12-22

- ASB: The `history` command will now display additional information about each swap such as the amounts involved, the current state and the txid of the Bitcoin lock transaction.

## [1.0.0-rc.10] - 2024-12-05

- GUI: Release .deb installer for Debian-based systems
- ASB: The maker will now retry indefinitely to redeem the Bitcoin until the cancel timelock expires. This fixes an issue where the swap would be refunded if the maker failed to publish the redeem transaction on the first try (e.g due to a network error).
- ASB (experimental): We now listen on an onion address by default using an internal Tor client. You do not need to run a Tor daemon on your own anymore. The `tor.control_port` and `tor.socks5_port` properties in the config file have been removed. A new `tor.register_hidden_service` property has been added which when set to `true` will run a hidden service on which connections will be accepted. You can configure the number of introduction points to use by setting the `tor.hidden_service_num_intro_points` (3 - 20) property in the config file. The onion address will be advertised to all rendezvous points without having to be added to `network.external_addresses`. For now, this feature is experimental and may be unstable. We recommend you use it in combination with a clearnet address. This feature is powered by [arti](https://tpo.pages.torproject.net/core/arti/), an implementation of the Tor protocol in Rust by the Tor Project.
- CLI + GUI: We can now dial makers over `/onion3/****` addresses using the integrated Tor client.

## [1.0.0-rc.7] - 2024-11-26

- GUI: Changed terminology from "swap providers" to "makers"
- GUI: For each maker, we now display a unique deterministically generated avatar derived from the maker's public key

## [1.0.0-rc.6] - 2024-11-21

- CLI + GUI: Tor is now bundled with the application. All libp2p connections between peers are routed through Tor, if the `--enable-tor` flag is set. The `--tor-socks5-port` argument has been removed. This feature is powered by [arti](https://tpo.pages.torproject.net/core/arti/), an implementation of the Tor protocol in Rust by the Tor Project.
- CLI + GUI: At startup the wallets and tor client are started in parallel. This will speed up the startup time of the application.

## [1.0.0-rc.5] - 2024-11-19

- GUI: Set new Discord invite link to non-expired one
- GUI: Fix an issues where asbs would not be sorted correctly
- ASB: Change level of logs related to rendezvous registrations to `TRACE`

## [1.0.0-rc.4] - 2024-11-17

- GUI: Fix an issue where the AppImage would render a blank screen on some Linux systems
- ASB: We now log verbose messages to hourly rotating `tracing*.log` which are kept for 24 hours. General logs are written to `swap-all.log`.

## [1.0.0-rc.2] - 2024-11-16

- GUI: ASBs discovered via rendezvous are now prioritized if they are running the latest version
- GUI: Display up to 16 characters of the peer id of ASBs

## [1.0.0-rc.1] - 2024-11-15

## [1.0.0-alpha.3] - 2024-11-14

## [1.0.0-alpha.2] - 2024-11-14

### **GUI**

- Display a progress bar to user while we are downloading the `monero-wallet-rpc`
- Release `.app` builds for Darwin

## [1.0.0-alpha.1] - 2024-11-14

- GUI: Swaps will now be refunded as soon as the cancel timelock expires if the GUI is running but the swap dialog is not open.
- Breaking change: Increased Bitcoin refund window from 12 hours (72 blocks) to 24 hours (144 blocks) on mainnet. This change affects the default transaction configuration and requires both CLI and ASB to be updated to maintain compatibility. Earlier versions will not be able to initiate new swaps with peers running this version.
- Breaking network protocol change: The libp2p version has been upgraded to 0.53 which includes breaking network protocol changes. ASBs and CLIs will not be able to swap if one of them is on the old version.
- ASB: Transfer proofs will be repeatedly sent until they are acknowledged by the other party. This fixes a bug where it'd seem to Bob as if the Alice never locked the Monero. Forcing the swap to be refunded.
- CLI: Encrypted signatures will be repeatedly sent until they are acknowledged by the other party
- ASB: We now retry indefinitely to lock Monero funds until the swap is cancelled. This fixes an issue where we would fail to lock Monero on the first try (e.g., due to the daemon not being fully synced) and would never try again, forcing the swap to be refunded.
- ASB + CLI: You can now use the `logs` command to retrieve logs stored in the past, redacting addresses and id's using `logs --redact`.
- ASB: The `--disable-timestamp` flag has been removed
- ASB: The `history` command can now be used while the asb is running.
- Introduced a cooperative Monero redeem feature for Bob to request from Alice if Bob is punished for not refunding in time. Alice can choose to cooperate but is not obligated to do so. This change is backwards compatible. To attempt recovery, resume a swap in the "Bitcoin punished" state. Success depends on Alice being active and still having a record of the swap. Note that Alice's cooperation is voluntary and recovery is not guaranteed
- CLI: `--change-address` can now be omitted. In that case, any change is refunded to the internal bitcoin wallet.

## [0.13.2] - 2024-07-02

- CLI: Buffer received transfer proofs for later processing if we're currently running a different swap
- CLI: We now display the reason for a failed cancel-refund operation to the user (#683)

## [0.13.1] - 2024-06-10

- Add retry logic to monero-wallet-rpc wallet refresh

## [0.13.0] - 2024-05-29

- Minimum Supported Rust Version (MSRV) bumped to 1.74
- Lowered default Bitcoin confirmation target for Bob to 1 to make sure Bitcoin transactions get confirmed in time
- Added support for starting the CLI (using the `start-daemon` subcommand) as a Daemon that accepts JSON-RPC requests
- Update monero-wallet-rpc version to v0.18.3.1

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

[unreleased]: https://github.com/UnstoppableSwap/core/compare/2.0.3...HEAD
[2.0.3]: https://github.com/UnstoppableSwap/core/compare/2.0.2...2.0.3
[2.0.2]: https://github.com/UnstoppableSwap/core/compare/2.0.0...2.0.2
[2.0.0]: https://github.com/UnstoppableSwap/core/compare/2.0.0-beta.2...2.0.0
[2.0.0-beta.2]: https://github.com/UnstoppableSwap/core/compare/2.0.0-beta.1...2.0.0-beta.2
[2.0.0-beta.1]: https://github.com/UnstoppableSwap/core/compare/1.1.7...2.0.0-beta.1
[1.1.7]: https://github.com/UnstoppableSwap/core/compare/1.1.4...1.1.7
[1.1.4]: https://github.com/UnstoppableSwap/core/compare/1.1.3...1.1.4
[1.1.3]: https://github.com/UnstoppableSwap/core/compare/1.1.2...1.1.3
[1.1.2]: https://github.com/UnstoppableSwap/core/compare/1.1.1...1.1.2
[1.1.1]: https://github.com/UnstoppableSwap/core/compare/1.1.0...1.1.1
[1.1.0]: https://github.com/UnstoppableSwap/core/compare/1.1.0-rc.3...1.1.0
[1.1.0-rc.3]: https://github.com/UnstoppableSwap/core/compare/1.1.0-rc.2...1.1.0-rc.3
[1.1.0-rc.2]: https://github.com/UnstoppableSwap/core/compare/1.1.0-rc.1...1.1.0-rc.2
[1.1.0-rc.1]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.21...1.1.0-rc.1
[1.0.0-rc.21]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.20...1.0.0-rc.21
[1.0.0-rc.20]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.19...1.0.0-rc.20
[1.0.0-rc.19]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.18...1.0.0-rc.19
[1.0.0-rc.18]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.17...1.0.0-rc.18
[1.0.0-rc.17]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.16...1.0.0-rc.17
[1.0.0-rc.16]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.14...1.0.0-rc.16
[1.0.0-rc.14]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.13...1.0.0-rc.14
[1.0.0-rc.13]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.12...1.0.0-rc.13
[1.0.0-rc.12]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.11...1.0.0-rc.12
[1.0.0-rc.11]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.10...1.0.0-rc.11
[1.0.0-rc.10]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.8...1.0.0-rc.10
[1.0.0-rc.8]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.7...1.0.0-rc.8
[1.0.0-rc.7]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.6...1.0.0-rc.7
[1.0.0-rc.6]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.5...1.0.0-rc.6
[1.0.0-rc.5]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.4...1.0.0-rc.5
[1.0.0-rc.4]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.2...1.0.0-rc.4
[1.0.0-rc.2]: https://github.com/UnstoppableSwap/core/compare/1.0.0-rc.1...1.0.0-rc.2
[1.0.0-rc.1]: https://github.com/UnstoppableSwap/core/compare/1.0.0-alpha.3...1.0.0-rc.1
[1.0.0-alpha.3]: https://github.com/UnstoppableSwap/core/compare/1.0.0-alpha.2...1.0.0-alpha.3
[1.0.0-alpha.2]: https://github.com/UnstoppableSwap/core/compare/1.0.0-alpha.1...1.0.0-alpha.2
[1.0.0-alpha.1]: https://github.com/UnstoppableSwap/core/compare/0.13.2...1.0.0-alpha.1
[0.13.2]: https://github.com/comit-network/xmr-btc-swap/compare/0.13.1...0.13.2
[0.13.1]: https://github.com/comit-network/xmr-btc-swap/compare/0.13.0...0.13.1
[0.13.0]: https://github.com/comit-network/xmr-btc-swap/compare/0.12.3...0.13.0
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
