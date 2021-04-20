#include <stdint.h>
#include "include/keccak.h"
#include "include/crypto-ops.h"

void hash_to_scalar(const uint8_t *in, uint8_t *md) {
    keccak(in, 32, md, 32);
    sc_reduce32(md);
}

// Hash a key to p3 representation
void hash_to_p3(const uint8_t *in, ge_p3 *hash8_p3) {
    uint8_t md[32];
    ge_p2 hash_p2;
    ge_p1p1 hash8_p1p1;

    keccak(in, 32, md, 32);
    ge_fromfe_frombytes_vartime(&hash_p2, md);
    ge_mul8(&hash8_p1p1, &hash_p2);
    ge_p1p1_to_p3(hash8_p3, &hash8_p1p1);
}

