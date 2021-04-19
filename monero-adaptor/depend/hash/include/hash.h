//
// Created by Rishab Sharma on 16/4/21.
//

#include <stdint.h>
#include <stdio.h>

#ifndef XMR_BTC_SWAP_COMIT_HASH_H
#define XMR_BTC_SWAP_COMIT_HASH_H

#ifndef KECCAK_ROUNDS
#define KECCAK_ROUNDS 24
#endif

enum {
  HASH_SIZE = 32,
  HASH_DATA_AREA = 136
};


void keccak(const uint8_t *in, size_t inlen, uint8_t *md, int mdlen);
void sc_reduce32(unsigned char *);

#endif //XMR_BTC_SWAP_COMIT_HASH_H
