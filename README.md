XMR to BTC Atomic Swap
======================

This repository is a proof of concept for atomically swapping XMR for BTC.

We define:

- Alice to be the actor that initially holds XMR.
- Bob to be the actor that initially holds BTC.

In the best-case scenario the protocol looks like this:

1. Alice and Bob exchange a set of addresses, keys, zero-knowledge proofs and signatures.
2. Bob publishes `Tx_lock`, locking up his bitcoin in a 2-of-2 multisig output owned by Alice and Bob.
Given the information exchanged in step 1, Bob can refund his bitcoin if he waits until time `t_1` by using `Tx_cancel` and `Tx_refund`.
If Bob doesn't refund after time `t_1`, Alice can punish Bob for being inactive by first publishing `Tx_cancel` and, after `t_2`, spending the output using `Tx_punish`.
3. Alice sees that Bob has locked up the bitcoin, so she publishes `Tx_lock` on the Monero blockchain, locking up her monero in an output which can only be spent with a secret key owned by Alice (`s_a`) *and* a secret key owned by Bob (`s_b`).
This means that neither of them can actually spend this output unless they learn the secret key of the other party.
4. Bob sees that Alice has locked up the monero, so he now sends Alice a missing key bit of information which will allow Alice to redeem the bitcoin using `Tx_redeem`.
5. Alice uses this information to spend the bitcoin to an address owned by her.
When doing so she leaks her Monero secret key `s_a` to Bob through the magic of adaptor signatures.
6. Bob sees Alice's `Tx_redeem` on Bitcoin, extracts Alice's secret key from it and combines it with his own to spend the monero to an address of his own.

![BTC/XMR atomic swap protocol](https://github.com/comit-network/xmr-btc-swap/blob/readme/BTC_XMR_atomic_swap_protocol.svg)

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
- It is possible to interact with, and watch, the monero blockchain using `monero-wallet-rpc`.
- It is possible to watch a bitcoind instance using `bitcoin-harness` (we already knew this :)

Currently we do not do:

- Actual network communication.
- Verification that the UI is acceptable.
Since we do everything in a single test function there is no user interaction, this is unrealistic for a real product.
  

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

