#!/bin/bash

CLI_PATH=$1
YOUR_MONERO_ADDR=$2
YOUR_BITCOIN_ADDR=$3

CLI_LIST_SELLERS="$CLI_PATH --testnet --json --debug list-sellers"
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
