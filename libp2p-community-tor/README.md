[![Continuous integration](https://github.com/umgefahren/libp2p-tor/actions/workflows/ci.yml/badge.svg)](https://github.com/umgefahren/libp2p-tor/actions/workflows/ci.yml)
[![docs.rs](https://img.shields.io/docsrs/libp2p-community-tor?style=flat-square)](https://docs.rs/libp2p-community-tor/latest)
[![Crates.io](https://img.shields.io/crates/v/libp2p-community-tor?style=flat-square)](https://crates.io/crates/libp2p-community-tor)

# libp2p Tor

Tor based transport for libp2p. Connect through the Tor network to TCP listeners.

Build on top of [Arti](https://gitlab.torproject.org/tpo/core/arti).

## New Feature

This crate supports, since #21 (thanks to @binarybaron), listening as a Tor hidden service as well as connecting to them.

## ⚠️ Misuse warning ⚠️ - read carefully before using

Although the sound of "Tor" might convey a sense of security it is _very_ easy to misuse this
crate and leaking private information while using. Study libp2p carefully and try to make sure
you fully understand it's current limits regarding privacy. I.e. using identify might already
render this transport obsolete.

This transport explicitly **doesn't** provide any enhanced privacy if it's just used like a regular transport.
Use with caution and at your own risk. **Don't** just blindly advertise Tor without fully understanding what you
are dealing with.

### Add to your dependencies

```bash
cargo add libp2p-community-tor
```

This crate uses tokio with rustls for its runtime and TLS implementation.
No other combinations are supported.

- [`rustls`](https://github.com/rustls/rustls)
- [`tokio`](https://github.com/tokio-rs/tokio)

### Example

```rust
let address = "/dns/www.torproject.org/tcp/1000".parse()?;
let mut transport = libp2p_community_tor::TorTransport::bootstrapped().await?;
// we have achieved tor connection
let _conn = transport.dial(address)?.await?;
```

### About

This crate originates in a PR to bring Tor support too rust-libp2p. Read more about it here: libp2p/rust-libp2p#2899

License: MIT
