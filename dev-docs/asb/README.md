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

Public Monero nodes for running the Monero Wallet RPC on stagenet can be found [here](https://monero.fail/?chain=monero). Public Electrum nodes for testnet can be found [here](https://1209k.com/bitcoin-eye/ele.php?chain=tbtc).

Run `./asb --help` for more information.

### Running on mainnet

For running the ASB on mainnet you will have to change you `monero-wallet-rpc` setup to mainnet.

It is recommended that you run your own Monero and Bitcoin node when running on mainnet.
It is possible to plug into public blockchain nodes but be aware that you might lose some privacy doing so.
Public Monero mainnet nodes can be found [here](https://monero.fail/?chain=monero).
Public Electrum mainnet nodes can be found [here](https://1209k.com/bitcoin-eye/ele.php?chain=btc).

### Connect with others

Consider joining the designated [Matrix chat](https://matrix.to/#/%23unstoppableswap-market-makers:matrix.org) to connect with other individuals running asbs. The core developers are active in this chat and always looking for feedback.

### Using Docker

Running the ASB and its required services (Bitcoin node, Monero node, wallet RPC) can be complex to set up manually.
We provide a Docker Compose solution that handles all of this automatically.
See our [docker-compose repository](https://github.com/UnstoppableSwap/asb-docker-compose) for setup instructions and configuration details.

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
  "/dns4/discover2.unstoppableswap.net/tcp/8888/p2p/12D3KooWGRvf7qVQDrNR5nfYD6rKrbgeTi9x8RrbdxbmsPvxL4mw",
  "/dns4/darkness.su/tcp/8888/p2p/12D3KooWFQAgVVS9t9UgL6v1sLprJVM7am5hFK7vy9iBCCoCBYmU",
]
external_addresses = ["/dns4/example.com/tcp/9939"]
```

For more information on the concept of multiaddresses, check out the libp2p documentation [here](https://docs.libp2p.io/concepts/addressing/).
In particular, you may be interested in setting up your ASB to be reachable via a [`/dnsaddr`](https://github.com/multiformats/multiaddr/blob/master/protocols/DNSADDR.md) multiaddress.
`/dnsaddr` addresses provide you with flexibility over the port and also allow you to register two addresses with transports (with and without websockets for example) under the same name.

### Setup Details

In order to understand the different components of the ASB and CLI better here is a component diagram showcasing the ASB and CLI setup using public Bitcoin and Monero infrastructure:

![Service Provider scenarios](http://www.plantuml.com/plantuml/proxy?cache=no&src=https://raw.githubusercontent.com/UnstoppableSwap/core/refs/heads/master/dev-docs/asb/diagrams/cli-asb-components-asb-pub-nodes.puml)

Contrary, here is a diagram that showcases a service provider running it's own blockchain infrastructure for the ASB:

![Service Provider scenarios](http://www.plantuml.com/plantuml/proxy?cache=no&src=https://raw.githubusercontent.com/UnstoppableSwap/core/refs/heads/master/dev-docs/asb/diagrams/cli-asb-components-asb-self-hosted.puml)

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
external_bitcoin_address = "bc1..."
```

The minimum and maximum amount as well as a spread, that is added on top of the price fetched from a central exchange, can be configured.

`external_bitcoin_address` allows to specify the Bitcoin address that the ASB will use to redeem or punish swaps.
If the option is not set, a new address from the internal wallet is used for every swap.

In order to be able to trade, the ASB must define a price to be able to agree on the amounts to be swapped with a CLI.
The `XMR<>BTC` price is currently determined by the price from the central exchange Kraken.
Upon startup the ASB connects to the Kraken price websocket and listens on the stream for price updates.
You can plug in a different price ticker websocket using the `price_ticker_ws_url` configuration option.
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

If `tor.register_hidden_service` is set to `true` that asb will automatically start listening on an onion service.

### Exporting the Bitcoin wallet descriptor

First use `swap` or `asb` with the `export-bitcoin-wallet` subcommand.

Output example:

```json
{
  "descriptor": "wpkh(tprv8Zgredacted.../84'/1'/0'/0/*)",
  "blockheight": 2415616,
  "label": "asb-testnet"
}
```

The wallet can theoretically be directly imported into
[bdk-cli](https://bitcoindevkit.org/bdk-cli/installation/) but it is easier to
use Sparrow Wallet.

Sparrow wallet import works as follows:

- File -> New wallet -> Give it a name
- Select "New or Imported Software Wallet"
- Click "Enter Private Key" for "Master Private Key (BIP32)"
- Enter the `xprv...` or `tprv...` part of the descriptor (example above is `tprv8Zgredacted...`:

![image](enter-master-private-key.png)

- Click "Import"
- Leave the derivation path as `m/84'/0'/0'` and click "Import Keystore" button
- Click "Apply" and then supply password

![image](import-keystore.png)

- Click Transactions tab
- ???
- Profit!

![image](transactions-tab.png)

If the bitcoin amount in your wallet doesn't match "asb balance" output and you don't see (all) the transactions you need to increase the gap limit:

- go to Settings > Advanced... > Gap limit

![image](gap-limit.png)
