# XMR to BTC Atomic Swap - Automated Swap Backend (ASB)

## Quick Start ASB

1. Download [latest release](https://github.com/comit-network/xmr-btc-swap/releases/latest) of the `asb` binary
2. Ensure that you have the Monero Wallet RPC running with `--wallet-dir` and `--disable-rpc-login`:
   1. `monero-wallet-rpc --stagenet --daemon-host STAGENET-NODE-URL --rpc-bind-port STAGENET-NODE-PORT --disable-rpc-login --wallet-dir PATH/TO/WALLET/DIR`
3. Run the ASB in terminal: `./asb start`
4. Follow the setup wizard in the terminal

Public Monero stagenet nodes for running the Monero Wallet RPC:

- `monero-stagenet.exan.tech:38081`
- `stagenet.community.xmr.to:38081`

Run `./asb --help` for more information.

## ASB Details

The ASB is a long running daemon that acts as the trading partner to the swap CLI.
The CLI user is buying XMR (i.e. receives XMR, sends BTC), the ASB service provider is selling XMR (i.e. sends XMR, receives BTC).
The ASB can handle multiple swaps with different peers concurrently.
The ASB communicates with the CLI on various [libp2p](https://libp2p.io/)-based network protocols.

Both the ASB and the CLI can be run by anybody.
The CLI is designed to run one specific swap against an ASB.
The ASB is designed to run 24/7 as a daemon that responds to CLIs connecting.
Since the ASB is a long running task we specify the person running an ASB as service provider.

### ASB discovery

Currently, there is no automated discovery for service providers running an ASB.
A service provider has to manually provide the connection details to users that will run the CLI.

[Libp2p addressing](https://docs.libp2p.io/concepts/addressing/) is used to identify a service provider by multi-address and peer-id.
The Peer-ID is printed upon startup of the ASB.
The multi-address typically consists of IP-address or URL (if DNS entry configured) of the service provider.

When configuring a domain name for the ASB through a DNS entry, a service provider can configure it by using the [`dnsaddr` format](https://github.com/multiformats/multiaddr/blob/master/protocols/DNSADDR.md) for the TXT entry.
This will simplify the connection detail `--seller-addr` for CLI users connecting to the ASB and provides more flexibility with e.g. ports (i.e. `/dnsaddr/your.domain.tld` instead of `/dns4/your.domain.tld/tcp/port`).

Each service provider running an ASB can decide how/where to share these connection details.

![Service Provider scenarios](http://www.plantuml.com/plantuml/proxy?cache=no&src=https://raw.githubusercontent.com/comit-network/xmr-btc-swap/d2cf45d8b9f0c2e180cd85aa034f370965adc11c/docs/asb/diagrams/cli-asb-overview.puml)

Eventually, more elaborate discovery mechanisms can be added.

The **CLI** user can specify a service providers's multiaddress and peer-id with `--seller-addr` and `--seller-peer-id`, see `./swap --help` for details.

### Setup Details

In order to understand the different components of the ASB and CLI better here is a component diagram showcasing the ASB and CLI setup using public Bitcoin and Monero infrastructure:

![Service Provider scenarios](http://www.plantuml.com/plantuml/proxy?cache=no&src=https://raw.githubusercontent.com/comit-network/xmr-btc-swap/363ce1cdf6fe6478736ff91e1458d650c2319248/docs/asb/diagrams/cli-asb-components-asb-pub-nodes.puml)

Contrary, here is a diagram that showcases a service provider running it's own blockchain infrastructure for the ASB:

![Service Provider scenarios](http://www.plantuml.com/plantuml/proxy?cache=no&src=https://raw.githubusercontent.com/comit-network/xmr-btc-swap/363ce1cdf6fe6478736ff91e1458d650c2319248/docs/asb/diagrams/cli-asb-components-asb-self-hosted.puml)

The diagram shows that the `asb` group (representing the `asb` binary) consists of three components:

1. Monero Wallet
2. Bitcoin Wallet
3. ASB

The `ASB` depicted in the diagram actually consists of multiple components (protocol impl, network communication, ...) that sums up the functionality to execute concurrent swaps in the role of Alice.

#### Monero Wallet Setup

The ASB uses the running Monero wallet RPC to create / open Monero wallets.
Currently you cannot connect to an existing Monero wallet, but the ASB will create the wallet `asb-wallet` upon intial startup.
In order to accept trades with a CLI you will have to send XMR to that wallet.
The wallet's address is printed upon startup of the ASB.
Currently the `asb-wallet` does not have a password.

Upon startup of the ASB the `asb-wallet` is opened in the wallet RPC.
You can then interact with the wallet RPC for basic wallet management as well.

#### Bitcoin Wallet Setup

The ASB has an internally managed Bitcoin wallet.
The Bitcoin wallet is created upon initial startup and stored in the data folder of the ASB (configured through initial startup wizard).

#### Market Making

In order to be able to trade, the ASB must define a price to be able to agree on the amounts to be swapped with a CLI.
Currently we use a spot-price mode, i.e. the ASB dictates the price to the CLI.

A CLI can connect to the ASB at any time and request a quote for buying XMR.
The ASB then returns the current price and the maximum amount tradeable.

The maximum amount tradeable can be configured with the `--max-buy-btc` parameter.

The `XMR<>BTC` price is currently determined by the price from the central exchange Kraken.
Upon startup the ASB connects to the Kraken price websocket and listens on the stream for price updates.

Currently the spot price is equal to the market price on Kraken.

#### Swap Execution

Swap execution within the ASB is automated.
Incoming swaps request will be automatically processed and the swap will execute automatically.
Swaps where Bob does not act, so Alice cannot redeem, will be automatically refunded or punished.
When the ASB is restarted unfinished swaps will be resumed automatically.

The refund scenario is a scenario where the CLI refunds the Bitcoin.
The ASB can then refund the Monero which will be automatically transferred to the `asb-wallet`.

The punish scenario is a scenario where the CLI does not refund and hence the ASB cannot refund the Monero.
After a second timelock expires the ASB will automatically punish the CLI user by taking the Bitcoin.

More information about the protocol in this [presentation](https://youtu.be/Jj8rd4WOEy0) and this [blog post](https://comit.network/blog/2020/10/06/monero-bitcoin).

All claimed Bitcoin ends up in the internal Bitcoin wallet of the ASB.
The ASB offers a commands to withdraw Bitcoin and check the balance, run `./asb --help` for details.

If the ASB has insufficient Monero funds to accept a swap the swap setup is rejected.
Note that currently there is no specific error sent back to the CLI for such kind of cases, so a user might not know why the swap execution was rejected.
Note that there is currently no notification service implemented for low funds.
The ASB provider has to monitor Monero funds to make sure the ASB still has liquidity.

#### Tor and hidden services

The ASB supports will automatically create a Tor hidden service if the Tor control port can be found.
By default, the ASB will look for the control port under `localhost:9051`.
To allow the ASB to create hidden services, enable the control port and authentication in your torrc file.
Concretely, add these lines:

```
ControlPort 9051
CookieAuthentication 1
CookieAuthFileGroupReadable 1
```

It is important that the user running the ASB has the correct user rights, i.e. is in the same group as the user running Tor.
E.g. if running on debian and having Tor install via apt, add your user to the following group:
`sudo adduser $(whoami) debian-tor`.
When configured correctly, your ASB will print the created onion addresses:

```bash
./bin/asb start
May 01 01:31:27.602  INFO Initialized tracing with level: debug
...
May 01 01:32:05.018  INFO Tor found. Setting up hidden service.
May 01 01:32:07.475  INFO /onion3/z4findrdwtfbpoq64ayjtmxvr52vvxnsynerlenlfkmm52dqxsl4deyd:9939
May 01 01:32:07.476  INFO /onion3/z4findrdwtfbpoq64ayjtmxvr52vvxnsynerlenlfkmm52dqxsl4deyd:9940
```
