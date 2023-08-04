# Automated Swap Backend (ASB)

## Quick Start

From version `0.6.0` onwards the software default to running on `mainnet`.
It is recommended to try the software on testnet first, which can be achieved by providing the `--testnet` flag.
This quickstart guide assumes that you are running the software on testnet (i.e. Bitcoin testnet3 and Monero stagenet):

1. Download [latest release](https://github.com/comit-network/xmr-btc-swap/releases/latest) of the `asb` binary
2. Ensure that you have the Monero Wallet RPC running with `--wallet-dir` and `--disable-rpc-login`:
   1. `monero-wallet-rpc --stagenet --daemon-host STAGENET-NODE-URL --rpc-bind-port STAGENET-NODE-PORT --disable-rpc-login --wallet-dir PATH/TO/WALLET/DIR`
3. Run the ASB in terminal: `./asb --testnet start`
4. Follow the setup wizard in the terminal

Public Monero nodes for running the Monero Wallet RPC can be found [here](https://community.rino.io/nodes.html).

Run `./asb --help` for more information.

### Running on mainnet

For running the ASB on mainnet you will have to change you `monero-wallet-rpc` setup to mainnet.

It is recommended that you run your own Monero and Bitcoin node when running on mainnet.
It is possible to plug into public blockchain nodes but be aware that you might lose some privacy doing so.
Public Monero mainnet nodes can be found [here](https://moneroworld.com/#nodes).
Public Electrum mainnet nodes can be found [here](https://1209k.com/bitcoin-eye/ele.php?chain=btc).

## ASB Details

The ASB is a long running daemon that acts as the trading partner to the swap CLI.
The CLI user is buying XMR (i.e. receives XMR, sends BTC), the ASB service provider is selling XMR (i.e. sends XMR, receives BTC).
The ASB can handle multiple swaps with different peers concurrently.
The ASB communicates with the CLI on various [libp2p-based](https://libp2p.io/) network protocols.

Both the ASB and the CLI can be run by anybody.
The CLI is designed to run one specific swap against an ASB.
The ASB is designed to run 24/7 as a daemon that responds to CLIs connecting.
Since the ASB is a long running task we specify the person running an ASB as service provider.

### ASB discovery

The ASB daemon supports the libp2p [rendezvous-protocol](https://github.com/libp2p/specs/tree/master/rendezvous).
Usage of the rendezvous functionality is entirely optional.

You can configure one or more rendezvous points in the `[network]` section of your config file.
For the registration to be successful, you also need to configure the externally reachable addresses within the `[network]` section.
For example:

```toml
[network]
rendezvous_point = [
   "/dns4/discover.unstoppableswap.net/tcp/8888/p2p/12D3KooWA6cnqJpVnreBVnoro8midDL9Lpzmg8oJPoAGi7YYaamE",
   "/dns4/eratosthen.es/tcp/7798/p2p/12D3KooWAh7EXXa2ZyegzLGdjvj1W4G3EXrTGrf6trraoT1MEobs",
]
external_addresses = ["/dns4/example.com/tcp/9939"]
```

For more information on the concept of multiaddresses, check out the libp2p documentation [here](https://docs.libp2p.io/concepts/addressing/).
In particular, you may be interested in setting up your ASB to be reachable via a [`/dnsaddr`](https://github.com/multiformats/multiaddr/blob/master/protocols/DNSADDR.md) multiaddress.
`/dnsaddr` addresses provide you with flexibility over the port and also allow you to register two addresses with transports (with and without websockets for example) under the same name.

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

For market making the ASB offers the following parameters in the config:

```toml
[maker]
min_buy_btc = 0.0001
max_buy_btc = 0.0001
ask_spread = 0.02
price_ticker_ws_url = "wss://ws.kraken.com"
```

The minimum and maximum amount as well as a spread, that is added on top of the price fetched from a central exchange, can be configured.

In order to be able to trade, the ASB must define a price to be able to agree on the amounts to be swapped with a CLI.
The `XMR<>BTC` price is currently determined by the price from the central exchange Kraken.
Upon startup the ASB connects to the Kraken price websocket and listens on the stream for price updates.
You can plug in a different price ticker websocket using the the `price_ticker_ws_url` configuration option.
You will have to make sure that the format returned is the same as the format used by Kraken.

Currently, we use a spot-price model, i.e. the ASB dictates the price to the CLI.
A CLI can connect to the ASB at any time and request a quote for buying XMR.
The ASB then returns the current price and the minimum and maximum amount tradeable.

#### Swap Execution

Swap execution within the ASB is automated.
Incoming swaps request will be automatically processed, and the swap will execute automatically.
Swaps where Bob does not act, so Alice cannot redeem, will be automatically refunded or punished.
If the ASB is restarted unfinished swaps will be resumed automatically.

The refund scenario is a scenario where the CLI refunds the Bitcoin.
The ASB can then refund the Monero which will be automatically transferred back to the `asb-wallet`.

The punish scenario is a scenario where the CLI does not refund and hence the ASB cannot refund the Monero.
After a second timelock expires the ASB will automatically punish the CLI user by taking the Bitcoin.

More information about the protocol in this [presentation](https://youtu.be/Jj8rd4WOEy0) and this [blog post](https://comit.network/blog/2020/10/06/monero-bitcoin).

All claimed Bitcoin ends up in the internal Bitcoin wallet of the ASB.
The ASB offers a commands to withdraw Bitcoin and check the balance, run `./asb --help` for details.

If the ASB has insufficient Monero funds to accept a swap the swap setup is rejected.
Note that there is currently no notification service implemented for low funds.
The ASB provider has to monitor Monero funds to make sure the ASB still has liquidity.

#### Tor and hidden services

The ASB supports Tor and will automatically create a Tor hidden service if the Tor control port can be found.
By default, the ASB will look for the control port under `localhost:9051`.
To allow the ASB to create a hidden service, enable the control port and authentication in your torrc file:

```
ControlPort 9051
CookieAuthentication 1
CookieAuthFileGroupReadable 1
```

It is important that the user running the ASB has the correct user rights, i.e. is in the same group as the user running Tor.
E.g. if running on debian and having Tor install via apt, add your user to the following group:
`sudo adduser $(whoami) debian-tor`.
When configured correctly, your ASB will print the created onion addresses upon startup:

```bash
./bin/asb start
May 01 01:31:27.602  INFO Initialized tracing with level: debug
...
May 01 01:32:05.018  INFO Tor found. Setting up hidden service.
May 01 01:32:07.475  INFO /onion3/z4findrdwtfbpoq64ayjtmxvr52vvxnsynerlenlfkmm52dqxsl4deyd:9939
May 01 01:32:07.476  INFO /onion3/z4findrdwtfbpoq64ayjtmxvr52vvxnsynerlenlfkmm52dqxsl4deyd:9940
```
