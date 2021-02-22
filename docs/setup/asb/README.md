# Setting up the ASB

Setup guidelines for the automated swap backend.

## systemd services

We configure two pathes for starting the asb through systemd. 

Upon system startup we first start the monero wallet RPC and then start the asb using two services:

```
start-monero-wallet-rpc.service 
|-> start-asb.service
```

We trigger re-building the asb from source every 24 hours through the `pull-and-build-asb` timer and service.
The watcher-service `watch-asb-binary-change.path ` monitors the asb binary changing, and if changed triggers `restart-asb.service` that will restart `start-asb.service`:

```
pull-and-build-asb.timer 
|-> pull-and-build-asb.service 
    |-> watch-asb-binary-change.path 
        |-> restart-asb.service 
            |-> start-asb.service
```
