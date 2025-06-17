# monero-sys

This crate is an idiomatic wrapper around [`wallet2_api.h`](./monero/src/wallet/api/wallet2_api.h) from the official Monero codebase.
The C++ library is statically linked into the crate.

Since we statically link the Monero codebase, we need to build it.
That requires build dependencies, for a complete and up-to-date list see the Monero [README](./monero/README.md#dependencies).
Missing dependencies will currently result in obscure CMake or linker errors.
If you get obscure linker CMake or linker errors, check whether you correctly installed the dependencies.

Since we build the Monero codebase from source, building this crate for the first time might take a while.

## Contributing

Make sure to load the Monero submodule:

```bash
git submodule update --init --recursive
```
