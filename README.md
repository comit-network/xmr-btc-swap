XMR to BTC Atomic Swap
======================

This repository hosts an MVP for atomically swapping BTC to XMR.
It implements the protocol described in [this](https://arxiv.org/abs/2101.12332) paper.

## Quick start

1. Download the [latest release](https://github.com/comit-network/xmr-btc-swap/releases/latest) for your operation system
2. Run the binary: `./swap buy-xmr --receive-address <YOUR MONERO ADDRESS>`
3. Follow the instructions printed to the terminal

## Limitations

For now, the MVP is limited to `testnet3` on Bitcoin and `stagenet` on Monero.

## How it works

This repository primarily hosts two components:

- the `swap` CLI
- the `asb` service

### swap CLI

The `swap` CLI acts in the role of Bob and swaps BTC for XMR.
See `./swap --help` for a description of all commands.
The main command is `buy-xmr` which automatically connects to an instance of `asb`.

### asb service

`asb` is short for **a**utomated **s**wap **b**ackend (we are open to suggestions for better names!).
The service acts as the counter-party for the `swap` CLI in the role of Alice.
It provides the CLI with a quote and the liquidity necessary for swapping BTC into XMR.

## Contact

Feel free to reach to out us in the [COMIT-Monero Matrix channel](https://matrix.to/#/#comit-monero:matrix.org). 
