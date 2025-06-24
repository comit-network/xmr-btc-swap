# UnstoppableSwap

This is the monorepo containing the source code for all of our core projects:

- [`swap`](swap/README.md) contains the source code for the main swapping binaries, `asb` and `swap`
  - [`maker`](dev-docs/asb/README.md)
  - [`taker`](dev-docs/cli/README.md)
- [`gui`](src-gui/README.md) contains the new tauri based user interface
- [`tauri`](src-tauri/) contains the tauri bindings between binaries and user interface
- and other crates we use in our binaries

If you're just here for the software, head over to the [releases](https://github.com/UnstoppableSwap/xmr-btc-swap/releases/latest) tab and grab the binary for your operating system! If you're just looking for documentation, check out our [docs page](https://docs.unstoppableswap.net/) or our [github docs](dev-docs/README.md).

Join our [Matrix room](https://matrix.to/#/#unstoppableswap-core:matrix.org) to follow development more closely.

![Screenshot 2024-11-21 at 6 19 03 PM](https://github.com/user-attachments/assets/a9fe110e-90b4-4af8-8980-d4207a5e2a71)

## Contributing

We have a `justfile` containing a lot of useful commands.
Run `just help` to see all the available commands.

## Running tests

This repository uses [cargo-nextest](https://nexte.st/docs/running/) to run the
test suite.

```bash
cargo install cargo-nextest
cargo nextest run
```
