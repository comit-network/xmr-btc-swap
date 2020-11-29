# XMR<>BTC Atomic Swap - User Interface Prototype Validation

This document:

1. Collects the validation criteria.
2. Lists the created user interface prototypes, and link to Figma.
3. Maps the protoypes to the validation criteria.

This document will be updated with new information during the course of the project.

## Questions

The questions are split between `M`aker (liquidity provider) and `T`aker (normal user), because the objectives are somewhat different.

|  **Topic** | **High Level Questions** | **More specific question** | **User is happy to...** | **Actor** |
| --- | --- | --- | --- | --- |
|  Node & Wallet Management | How do users monitor the Bitcoin and Monero blockchain? | Is a user fine with trusting a third party to monitor transactions? | use a service like blockchain.com to retrieve blocks for validation | TM |
|   |  |  | run his own Bitcoin node, third party service for Monero | TM |
|   |  |  | run his own Monero node, third party service for Bitcoin | TM |
|   |  |  | run both his own Bitcoin and Monero node | TM |
|   | How do users brodcast transactions to Bitcoin and Monero? | Is a user fine with trusting a third party to broadcast transactions? | use a wallet that connects to third party nodes | TM |
|   |  |  | send signed transactions through third part nodes | TM |
|   |  |  | run his own blockchain full node | TM |
|   |  |  | run an SPV node (Bitcoin) | TM |
|   | How do users manage their wallets to interace with other software? | Do users want to use already existing wallets? | fund and redeem from existing wallets | TM |
|   |  |  | fund from existing Monero wallet, redeem to new Bitcoin wallet | TM |
|   |  |  | fund from existing Bitcoin wallet, redeem to new Monero wallet | TM |
|   |  |  | fund and redeem into new wallets (explicitly used for swap execution) | TM |
|   |  | What level of control does the user give to the execution daemon? | give the execution daemon control over the wallets (no user interaction, fully automated) | TM |
|   |  |  | use a Bitcoin transaction to give funds to the swap application | TM |
|   |  |  | use a Monero transaction to give funds to the swap application | TM |
|   |  |  | explicitly sign each transaction | TM |
|  Discovery | How do users discover trading partners? | Do users care about privacy? | go to website and take price from there | T |
|   |  |  | set up website (publicly) to advertise price (and connection information) | M |
|   |  |  | open "random" (tor) website found on various media (forums, chat) to access a single market maker. | T |
|   |  |  | configure Tor for trading | TM |
|   |  | Do users care about P2P? | use a centralized service to find makers | TM |
|   |  |  | user a decentralized service to find makers | TM |
|  Software Setup | How does the user want to manage the swap software setup? | Is the user willing to download software? | download software (swap execution daemon) before being able to do a swap | T |
|   |  | How does the user want to manage long-running tasks? | keep a GUI/CLI open for the whole length of the swap execution | T |
|   |  |  | keep a computer running (that hosts the daemon) for the whole length of the swap execution | T |
|   |  |  | keep the browser open for the whole length of a swap | T |
|  Protocol | How important are protocol details to the user? | Does the user care about the incentives of each role? | have different steps (locking first vs second) depending on the direction of the swap | TM |

## Prototypes

In the initial project description we distinguished product `A` a single market-maker product and product `B` a product including peer-to-peer discovery and multiple makers.

```
P ... Prototype that showcases a complete swap setup flow.
D ... Prototype that focuses on a specific detail of swap setup / execution.

{}-A ... Prototype for product A (single market maker)
{}-B ... Prototype for product B (decentralized trading platform)
```

Example:

`D-A2-1`: Mock showing detail 1 for prototype `P-A1`

### Figma Links

* [P-A1](https://www.figma.com/proto/QdvmbRYuBpEpFI3D1R4qyM/XMR-BTC_SingleMaker_LowFidelity?node-id=54%3A4894&viewport=1503%2C-52%2C0.5576764941215515&scaling=min-zoom): Webpage for discovery, CLI for execution
* [P-A2](https://www.figma.com/proto/QdvmbRYuBpEpFI3D1R4qyM/XMR-BTC_SingleMaker_LowFidelity?node-id=7%3A4377&viewport=696%2C-250%2C0.362735778093338&scaling=min-zoom): Webpage for discovery, GUI for execution
* [D-A2-1](https://www.figma.com/proto/QdvmbRYuBpEpFI3D1R4qyM/XMR-BTC_SingleMaker_LowFidelity?node-id=235%3A1374&viewport=1336%2C-1825%2C0.7878535389900208&scaling=min-zoom): GUI swap execution steps for `send` `BTC`, `receive` `XMR`
* [D-A2-2](https://www.figma.com/proto/QdvmbRYuBpEpFI3D1R4qyM/XMR-BTC_SingleMaker_LowFidelity?node-id=128%3A8016&viewport=1404%2C-1158%2C0.66261225938797&scaling=min-zoom): GUI swap execution steps for `send` `XMR`, `receive` `BTC`

### Mapping of Prototype to validation criteria

|  **User is happy to...** | **Actor** | **P-A1** | **P-A2** | **D-A2-1** | **D-A2-2** |
| --- | --- | --- | --- | --- | --- |
|  use a service like blockchain.com to retrieve blocks for validation | TM |  |  |  |  |
|  run his own Bitcoin node, third party service for Monero | TM |  |  |  |  |
|  run his own Monero node, third party service for Bitcoin | TM |  |  |  |  |
|  run both his own Bitcoin and Monero node | TM | T | T |  |  |
|  use a wallet that connects to third party nodes | TM |  |  |  |  |
|  send signed transactions through third part nodes | TM |  |  |  |  |
|  run his own blockchain full node | TM |  |  |  |  |
|  run an SPV node (Bitcoin) | TM |  |  |  |  |
|  fund and redeem from existing wallets | TM | T | T |  |  |
|  fund from existing Monero wallet, redeem to new Bitcoin wallet | TM |  |  |  |  |
|  fund from existing Bitcoin wallet, redeem to new Monero wallet | TM |  |  |  |  |
|  fund and redeem into new wallets (explicitly used for swap execution) | TM |  |  |  |  |
|  give the execution daemon control over the wallets (no user interaction, fully automated) | TM | T | T |  |  |
|  use a Bitcoin transaction to give funds to the swap application | TM |  |  |  |  |
|  use a Monero transaction to give funds to the swap application | TM |  |  |  |  |
|  explicitly sign each transaction | TM |  |  |  |  |
|  go to website and take price from there | T |  |  |  |  |
|  set up website (publicly) to advertise price (and connection information) | M | M | M |  |  |
|  open "random" (tor) website found on various media (forums, chat) to access a single market maker. | T |  |  |  |  |
|  configure Tor for trading | TM |  |  |  |  |
|  use a centralized service to find makers | TM | T | T |  |  |
|  user a decentralized service to find makers | TM |  |  |  |  |
|  download software (swap execution daemon) before being able to do a swap | T |  |  |  |  |
|  keep a GUI/CLI open for the whole length of the swap execution | T |  |  | T | T |
|  keep a computer running (that hosts the daemon) for the whole length of the swap execution | T | T | T | T | T |
|  keep the browser open for the whole length of a swap | T |  |  |  |  |
|  have different steps (locking first vs second) depending on the direction of the swap | TM |  |  | T (M) | T (M) |

Legend:

```
T ... Taker
M ... Maker
TM ... Taker and Maker
T (M) ... Taker showcased, Maker implicitly concerned as well
```

## Interviews

Through user interviews we plan to collect more information on the current setup of users, and how it could be used in a potential product.

Specific prototypes showcase specific answers to the questions listed above. We may use the prototypes in interviews to showcase scenarios.


## Feedback 

### Possible Features List

This section points out features that were mentioned by the community. These features will be evaluated and prioritized before we start building.

#### Avoid receiving tainted Bitcoin

Mentions:

* [27.11.2020 on Reddit](https://www.reddit.com/r/Monero/comments/k14hug/how_would_an_atomic_swap_ui_look_like/gdplnt8?utm_source=share&utm_medium=web2x&context=3)

The receiver of the Bitcoin should be able to validate the address to be used by the sender to avoid receiving tainted Bitcoin (i.e. coins that were unlawfully used). 
This feature is relevant for the receiving party of the Bitcoin, it is relevant for taker and maker.
This feature is relevant independent of the user use case.

In order to be able to spot tainted Bitcoin, the receiver has to validate the address to be used of the sender. 
In the current protocol the party funding (sending) Bitcoin always moves first.

The party receiving the Bitcoin would have to request the address to be used by the sender. 
For the beginning it might be good enough to let the taker verify that the Bitcoin are not tainted manually, eventually it can be done automated against a service listing tainted Bitcoin.
Once the daemon of the party receiving the Bitcoin sees the Bitcoin transaction of the sender, the address has to be evaluated to ensure the correct address has been used for funding.
This can be done automated.
In case a tainted address was used the swap execution should halt and give a warning to the receiving party.
