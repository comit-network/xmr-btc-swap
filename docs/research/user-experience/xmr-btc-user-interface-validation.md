# XMR<>BTC Atomic Swap - User Interface Prototype Validation

This document sums up the questions that we would like to validated with mocks.

## Questions

The questions are split between maker (liquidity provider) and taker (normal user), because the objectives are somewhat different.

Is a `taker` happy to...

1. run a bitcoind full-node
1. run a monerod full-node
1. use a webpage for discovering makers
1. open "random" (tor) website found on various media (forums, chat) to access a single market maker
1. manually add makers to retrieve orders
1. download software (swap execution daemon) before being able to do a swap
1. run the swap daemon locally on the taker's machine
1. keep a GUI open for the whole length of the swap execution
1. keep a computer running (that host the daemon) for the whole length of the swap execution
1. Swap funds held in monero-wallet (monerod)
1. Swap funds held in bitcoind wallet
1. give control over the bitcoin and monero wallets to the swap execution daemon
1. keep the browser open for the whole length of the swap execution
1. have different steps (locking first vs second) depending on the direction of the swap
1. to not use tor

... to do an XMR<>BTC swap?


Is a `maker` happy to...

1. run a bitcoind full-node
1. run a monerod full-node
1. set up a webpage where takers get a price 
1. publicly advertise being a maker
1. be in different cryptographic roles depending on the direction of the swap
1. run the swap execution daemon without using Tor
 
... to do an XMR<>BTC swap?



## Prototypes

This section lists the created user interface prototypes to the questions they should validate.

### A-1 Single-Maker Webpage and CLI 


### A-1 Single-Maker Webpage and CLI 
