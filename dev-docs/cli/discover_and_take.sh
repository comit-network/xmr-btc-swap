#!/bin/bash

# This is a utility script to showcase how the swap CLI can discover sellers and then trigger a swap using the discovered sellers
#
# 1st param: Path to the "swap" binary (aka the swap CLI)
# 2nd param: Multiaddress of the rendezvous node to be used for discovery
# 3rd param: Your Monero stagenet address where the XMR will be received
# 4th param: Your bech32 Bitcoin testnet address that will be used for any change output (e.g. refund scenario or when swapping an amount smaller than the transferred BTC)
#
# Example usage:
# discover_and_take.sh "PATH/TO/swap" "/dns4/discover.unstoppableswap.net/tcp/8888/p2p/12D3KooWA6cnqJpVnreBVnoro8midDL9Lpzmg8oJPoAGi7YYaamE" "YOUR_XMR_STAGENET_ADDRESS" "YOUR_BECH32_BITCOIN_TESTNET_ADDRESS"

CLI_PATH=$1
RENDEZVOUS_POINT=$2
YOUR_MONERO_ADDR=$3
YOUR_BITCOIN_ADDR=$4

CLI_LIST_SELLERS="$CLI_PATH --testnet --json --debug list-sellers --rendezvous-point $RENDEZVOUS_POINT"
echo "Requesting sellers with command: $CLI_LIST_SELLERS"
echo

BEST_SELLER=$($CLI_LIST_SELLERS | jq -s -c 'min_by(.status .Online .price)' | jq -r '.multiaddr, (.status .Online .price), (.status .Online .min_quantity), (.status .Online .max_quantity)')
read ADDR PRICE MIN MAX < <(echo $BEST_SELLER)

echo

echo "Seller with best price:"
echo "  multiaddr   : $ADDR"
echo "  price       : $PRICE sat"
echo "  min_quantity: $MIN sat"
echo "  max_quantity: $MAX sat"

echo

CLI_SWAP="$CLI_PATH --testnet --debug buy-xmr --receive-address $YOUR_MONERO_ADDR --change-address $YOUR_BITCOIN_ADDR --seller $ADDR"

echo "Starting swap with best seller using command $CLI_SWAP"
echo
$CLI_SWAP
