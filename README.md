# XMR to BTC Atomic Swap

This repository hosts an MVP for atomically swapping BTC to XMR.
It implements the protocol described in section 3 of [this](https://arxiv.org/abs/2101.12332) paper.

More information about the protocol in this [presentation](https://youtu.be/Jj8rd4WOEy0) and this [blog post](https://comit.network/blog/2020/10/06/monero-bitcoin).

## Quick start - CLI

1. Download the [latest `swap` binary release](https://github.com/comit-network/xmr-btc-swap/releases/latest) for your operating system
2. Run the binary specifying the monero address where you wish to receive monero and the connection details of the seller:
   `./swap buy-xmr --receive-address <YOUR MONERO ADDRESS> --seller-peer-id <SELLERS PEER ID> --seller-addr <SELLERS MULTIADDRESS>`
   You can generate a receive address using your monero wallet.
   The seller will provide you their peer id and multiaddress.
   We are running an `asb` instance on testnet.
   You can swap with to get familiar with the `swap` CLI.
   Our peer id is `12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi` and our multiaddress is `/dnsaddr/xmr-btc-asb.coblox.tech`
3. Follow the instructions printed to the terminal

## Limitations

For now, the MVP is limited to `testnet3` on Bitcoin and `stagenet` on Monero.

## How it works

This repository primarily hosts two components:

- the `swap` CLI
- the [`asb` service](/docs/asb/README.md)

### swap CLI

The `swap` CLI acts in the role of Bob and swaps BTC for XMR.
See `./swap --help` for a description of all commands.
The main command is `buy-xmr` which automatically connects to an instance of `asb`.

### asb service

`asb` is short for **a**utomated **s**wap **b**ackend (we are open to suggestions for better names!).
The service acts as the counter-party for the `swap` CLI in the role of Alice.
It provides the CLI with a quote and the liquidity necessary for swapping BTC into XMR.

For details on how to run the ASB please refer to the [ASB docs](/docs/asb/README.md).

## Contact

Feel free to reach out to us in the [COMIT-Monero Matrix channel](https://matrix.to/#/#comit-monero:matrix.org).
