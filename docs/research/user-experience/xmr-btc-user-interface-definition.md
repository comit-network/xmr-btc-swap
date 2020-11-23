# XMR<>BTC Atomic Swap - User Interface Prototype Definition

This document specifies assumptions concerning possible user interfaces on top of the [Monero-Bitcoin swap as described by Lucas on the COMIT-blog](https://comit.network/blog/2020/10/06/monero-bitcoin/).

We first specify assumptions and limitations imposed by the current setup and software needed for the swap, then outline possible solutions. 
This document will be used as a basis for defining and evaluating low-fidelity UI prototypes in the prototyping tool Figma.

## Assumptions

This section sums up assumptions around the current setup, protocol and existing software. 

### Security Assumptions

Swap-solutions that run completely in the browser (webpage) and rely on connecting to a locally running blockchain node are currently not possible because specifying [CORS headers](https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS) is a security risk.
This means the browser will reject a response from a locally running Bitcoin node.
Discussions have been ongoing, notably for [bitcoind](https://github.com/bitcoin/bitcoin/issues/11833), to add features that would allow webpages to communicate with locally running blockchain nodes directly.
However, it is hard to verify the code of a webpage when accessing the webpage in a browser and thus not recommended to add features that would allow to do this. 

Theoretically this could be overcome by creating a node infrastructure behind a proper REST API, or with a locally running proxy. However, it is questionable if these are favourable solution, as they adds additional software on top of the blockchain nodes.  

We will focus on solutions that favour the use of locally running blockchain nodes and explore solutions that do not access locally running blockchain nodes from a webpage directly.

### Current Setup

To be able to provide a reasonably trustless setup, where the user is in control of his private keys the user is currently advised to run his own Bitcoin and Monero node.

Given the current setup this results in the following software necessary to achieve a swap: 

* Bitcoin Blockchain Node
* Monero Blockchain Node
* Bitcoin Wallet
* Monero Wallet
* **Swap Execution Daemon**: Monitor the blockchain and move the swap forward to the next action.
* **Interface to connect the trading partners**: The two parties have to find each other and start the `Swap Execution Daemon` with the correct parameters. The interface can be realised in multiple different ways, from single-maker, P2P trading platform to P2P marketplace. 

For Bitcoin the current setup relies on a synced bitcoind node and bitcoind's wallet. 
For Monero the current setup relies on a synced monerod node and monero's wallet RPC. 
For funding Monero an existing wallet is unlocked and used for the swap.
For funding Bitcoin an existing wallet is unlocked and used for the swap. 
The swap execution daemon has control over the wallets during the swap execution to allow automated broadcasting of the transactions.

The current setup relies on generating a *new* Monero wallet for `redeeming` or `refunding`.
One trivial solution would be to add an additional transaction in the end, that transfers the Monero to the user's wallet.
More optimized solutions are possible as well, but require more research.
Since there are solutions to overcome the need of a second wallet we decided **not** to mock this part of the current setup in the UI prototypes.

### Current Protocol

The POC protocol is asymmetric and comes with several constraints that affect the user experience: 

1. The party that funds Bitcoin has to fund first.
2. The party that funds Bitcoin is always in the role of Bob.
3. There is an information exchange (Bob sends Alice a key-part for redeeming) during the execution of the swap which requires (P2P) communication between the parties.

Even though that is not per se limiting we have to acknowledge that the protocol will behave different for the maker and taker for the respective direction of the swap.

The protocol comes with different incentives for the two roles Alice and Bob. 
We think that, for a maker it is favourable to be in the role of Alice, because in Alice is in the powerful position of holding the secret. Additionally Bob funds first in the POC.
However, to be able to allow a product that offers XRM->BTC and BTC->XMR swaps, we decided to let the maker and taker be in either role.
The UX of the swap might be different depending on the role a party takes, especially when depicting the swap steps.

### Additional Assumptions

The first version of a prototype will not include hardware wallet (e.g. Ledger Nano S) support.
In previous products we focused on swaps directly from and to cold storage, however, this does not concern a large enough user base to support it in the initial version. 
Furthermore, hardware wallets add certain restrictions (forced user-interaction to sign transactions) that might conflict with the current prototol design.
Using the wallets of the standard implementations (monerod and bitcoind) is the easiest, most straightforward starting point.
Hardware wallet support can be added at a later point if requested as feature by the community.

## Possible Solutions

This section outlines possible solutions based on the assumptions stated above.
This section does not focus on the UX of the swap and the use-case specifics, but on the interaction between browser and other applications needed to achieve the swap.

We have explored the following options:

1. Extending existing (wallet) UIs (e.g. monero-wallet-gui) for swaps.
2. Webpage based swaps (completely running in browser).
3. Browser extension for swaps.
4. Desktop application for swaps.

Note that for desktop applications (4.) we explore the possibility of triggering the application through the browser.

### Extending existing (wallet) UIs

The nature of a swap requires management of both assets of the swap to be able to fund and redeem. 
This means, that a swap execution software has to interact with wallet software on both sides. 

Additionally, due to the complexity of the current swap protocol's setup and the fact that there is information exchange during the execution phase, it is highly questionable that communities on both sides would extend wallet functionality to allow swaps. 
For the time being the way forward seems to be a swap tool that plugs into existing wallet and blockchain node software on both sides, but handles additional requirements necessary for swap execution without modifying the existing software.

### Webpage

Allowing webpages access to a locally running 
A complete webpage based solution is not easily possible because of the CORS header restriction of browsers.
We could enable communication between blockchain nodes and the browser through e.g. a proxy, but such a solution would not have an advantage over running a desktop application, because either way one has to run additional software next to the blockchain nodes to enable the swap.

### Browser Extension

Browser extensions, are not as restrictive as webpages as can be seen in the [Google Chrome's documentsion](https://developer.chrome.com/extensions/xhr).
A browser extension would be able to offer a well-defined API, that allows multiple different web-sites to offer services within the extension.
This could range from discovery, negotiation to execution. 

Implementing a browser extension that handles the communication between maker and taker, and is able to communicate with locally running wallets and blockchain nodes is a valid solution.
We could also implement the decentralized orderbook on top of the browser extension, it is however, questionable if that would not be too much complexity packed into such kind of extension. 
We are still evaluating what is feasible for such an extension.
To start with a desktop application is simpler to prototype.

Mozilla's [web-ext tool](https://github.com/mozilla/web-ext) - besides [other tools](https://extensionworkshop.com/documentation/develop/browser-extension-development-tools/) - can help enable cross-browser extensions.

### Desktop Application

A desktop application is a valid solution that is not subject to restrictions by the browser.

The problem of finding a trading partner can be outsourced to a webpage (e.g. single-maker only).
Custom URL schemes can help to provide a better user experience, as they allow us to open the swap desktop application with parameters passed to the application through the customer URL. 
It can, however, be be quite a pain to achieve the proper registration of custom URL schemas for different kinds of operating systems.

A stand-alone desktop applications is possible, but can be seen as a hurdle, because every user first has to download the binary to see any form of user interface.
For complex applications that involve a decentralized orderbook a desktop application might be advisable for the moment.

For previous MVPs we used [Electron](https://www.electronjs.org/) to implement cross-platform desktop applications. 

## Use-case specific prototypes

Given the assumptions, technical limitations and resulting possible solutions described above this section specifies details for the prototypes described in the [initial project description](https://github.com/coblox/spikes/blob/master/0003-xmr-btc-product.md).
In the initial project description we define two products to be mocked as prototypes, (A) single market maker and (B) peer-to-peer decentralized trading platform. 

Note, that the use-cases of a single market maker product and a peer-to-peer decentralized trading platform are agnostic to the user interface.
Both products can be realised as CLI or UI taken into account the assumption listed above.

The "product specific prototypes" focus on the setup and interaction of different software. 
When it comes to prototyping specific user interface details, like for example the swap execution steps, we may opt for creating separate detail-mocks focussing on these details.

### A. Single Market Maker product

The point of this product is to keep `finding a trading partner` (discovery) and `agreeing on a trade` (negotiation) simple by only having a single maker. 
A single-maker runs a webpage that gives a price. A taker can "take it or leave it".

We plan to create two prototypes:

1. **Webpage + CLI:** The webpage prints a command that instructs the taker how to start the swap execution daemon.
2. **Webpage + UI:** The webpage uses a custom URL schema to trigger a desktop application passing the trading parameters to the application.

Additionally we might showcase a prototype that uses a webpage and browser-extension given that we can fit it into the project scope.

### B. Peer-to-peer decentralised trading platform

This product differs from product (A) by treating the discovery in a decentralized manner as well as allowing multiple makers.

The prototype will be modelled in a way, that keeps the negotiation (i.e. orderbook) simple.
The discovery shall be depicted as peer-to-peer.
It is planned to show specific orders of makers to takers, rather than prototyping a trading platform that includes order-matching.
The prototype will be modelled as desktop application that includes and controls the swap execution daemon.
