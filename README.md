XMR to BTC Atomic Swap
======================

This repository is a proof of concept for atomically swapping XMR for BTC.

We define:

- Alice to be the actor that initially holds XMR.
- Bob to be the actor that initially holds BTC.

The repository is structured as a library and a single test function that executes the swap.
The library has the following modules:

- `alice`: Defines the state machine that describes the swap for Alice.
This includes the messages sent to/from Alice.
- `bob`: Defines the state machine that describes the swap for Bob.
This includes the messages sent to/from Bob.
- `bitcoin`: Keys, signing functions, transactions etc. for Bitcoin.
Also includes a test wallet (see below).
- `monero`: Keys, signing functions, transactions etc. for Monero.
Also includes a test wallet (see below).

Currently we have a single test function that proves the following:

- Interaction with both block chains and their respective wallets works.
- The messages required are correct and can manually drive the state transitions to execute a swap.
  

Currently we do not do:

- Actual network communication.
- Watch the blockchain for transactions (we just assume they have been mined as soon as we broadcast and move onto the next state).
- Verification that the UI is acceptable.
Since we do everything in a single test function their is no user interaction, this is unrealistic for a real product.
  

## Testing

We wrote a few additional libraries to facilitate testing:

### Wallets

- `bitcoin` module contains a test wallet by way of `bitcoind`.
- `monero`: module contains a test wallet by way of `monero-wallet-rpc`.
  
### Blockchain harnesses

We have written two harnesses for interacting with bitcoin and monero.

- [bitcoin-harness](https://github.com/coblox/bitcoin-harness-rs)
- [monero-harness](https://github.com/comit-network/xmr-btc-swap/tree/master/monero-harness)

These harnesses wrap interaction with `bitcoind` and `monerod`/`monero-wallet-rpc`.

We use [testcontainers-rs](https://github.com/testcontainers/testcontainers-rs) to spin up `bitcoind`, `monerod`, and `monero-wallet-rpc` in docker containers during unit/integration testing.

