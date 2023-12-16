# XMR to BTC Atomic Swap

This repository hosts an MVP for atomically swapping BTC to XMR.
It implements the protocol described in section 3 of [this](https://arxiv.org/abs/2101.12332) paper.

More information about the protocol in this [presentation](https://youtu.be/Jj8rd4WOEy0) and this [blog post](https://comit.network/blog/2020/10/06/monero-bitcoin).

Currently, swaps are only offered in one direction with the `swap` CLI on the buying side (send BTC, receive XMR).
We are working on implementing a protocol where XMR moves first, but are currently blocked by advances on Monero itself.
You can read [this blogpost](https://comit.network/blog/2021/07/02/transaction-presigning) for more information.

## Quick Start

1. Download the [latest `swap` binary release](https://github.com/comit-network/xmr-btc-swap/releases/latest) for your operating system.
2. Find a seller to swap with:

```shell
./swap --testnet list-sellers
```

3. Swap with a seller:

```shell
./swap --testnet buy-xmr --receive-address <YOUR MONERO ADDRESS> --change-address <YOUR BITCOIN CHANGE ADDRESS> --seller <SELLER MULTIADDRESS>
```

For more detailed documentation on the CLI, see [this README](./docs/cli/README.md).

## Becoming a Market Maker

Swapping of course needs two parties - and the CLI is only one of them: The taker that occasionally starts a swap with a market maker.

If you are interested in becoming a market maker you will want to run the second binary provided in this repository: `asb` - the Automated Swap Backend.
Detailed documentation for the `asb` can be found [in this README](./docs/asb/README.md).

## Safety

This software is using cryptography that has not been formally audited.
While we do our best to make it safe, it is up to the user to evaluate whether or not it is safe to use for their purposes.
Please also see section 15 and 16 of the [license](./LICENSE).

Keep in mind that swaps are complex protocols, it is recommended to _not_ do anything fancy when moving coins in and out.
It is not recommended to bump fees when swapping because it can have unpredictable side effects.

## Contributing

We encourage community contributions whether it be a bug fix or an improvement to the documentation.
Please have a look at the [contribution guidelines](./CONTRIBUTING.md).

## Rust Version Support

Please note that only the latest stable Rust toolchain is supported.
All stable toolchains since 1.70 _should_ work.

## Contact

Feel free to reach out to us in the [COMIT-Monero Matrix channel](https://matrix.to/#/#comit-monero:matrix.org).
