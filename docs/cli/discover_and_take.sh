#!/bin/bash

CLI_PATH=$1
YOUR_MONERO_ADDR=$2

RENDEZVOUS_ADDR="/dnsaddr/rendezvous.coblox.tech"
RENDEZVOUS_PEER_ID="12D3KooWQUt9DkNZxEn2R5ymJzWj15MpG6mTW84kyd8vDaRZi46o"

# Since we always print json on stdout for `list-sellers` we don't need the `--json` flag
CLI_LIST_SELLERS="$CLI_PATH --testnet --debug list-sellers --rendezvous-node-peer-id $RENDEZVOUS_PEER_ID --rendezvous-node-addr $RENDEZVOUS_ADDR"
echo "Requesting sellers with command: $CLI_LIST_SELLERS"
echo

BEST_SELLER_ARR=$($CLI_LIST_SELLERS | jq -s -c 'sort_by(.quote .price)[]' | jq -r '.multiaddr, (.quote .price), (.quote .min_quantity), (.quote .max_quantity)')
read -a BEST_SELLER < <(echo $BEST_SELLER_ARR)

echo

echo "Seller with best price:"
echo "  multiaddr   : ${BEST_SELLER[0]}"
echo "  price       : ${BEST_SELLER[1]} sat"
echo "  min_quantity: ${BEST_SELLER[2]} sat"
echo "  max_quantity: ${BEST_SELLER[3]} sat"

echo

CLI_SWAP="$CLI_PATH --testnet --debug buy-xmr --receive-address $YOUR_MONERO_ADDR --seller-addr ${BEST_SELLER[0]}"

echo "Starting swap with best seller using command $CLI_SWAP"
echo
$CLI_SWAP
