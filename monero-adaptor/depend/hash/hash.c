#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include "hash.h"
#include <string.h>
#include <assert.h>

static void local_abort(const char *msg)
{
  fprintf(stderr, "%s\n", msg);
#ifdef NDEBUG
  _exit(1);
#else
  abort();
#endif
}

typedef uint64_t state_t[25];

void hash_to_scalar(const uint8_t *in, size_t inlen, uint8_t *md, int mdlen) {
  keccak(in, inlen, md, mdlen);
  sc_reduce32(md);
}

void keccak(const uint8_t *in, size_t inlen, uint8_t *md, int mdlen)
{
    state_t st;
    uint8_t temp[144];
    size_t i, rsiz, rsizw;

    static_assert(HASH_DATA_AREA <= sizeof(temp), "Bad keccak preconditions");
    if (mdlen <= 0 || (mdlen > 100 && sizeof(st) != (size_t)mdlen))
    {
      local_abort("Bad keccak use");
    }

    rsiz = sizeof(state_t) == mdlen ? HASH_DATA_AREA : 200 - 2 * mdlen;
    rsizw = rsiz / 8;

    memset(st, 0, sizeof(st));

    for ( ; inlen >= rsiz; inlen -= rsiz, in += rsiz) {
      for (i = 0; i < rsizw; i++) {
        uint64_t ina;
        memcpy(&ina, in + i * 8, 8);
        st[i] ^= swap64le(ina);
      }
      keccakf(st, KECCAK_ROUNDS);
    }

    // last block and padding
    if (inlen + 1 >= sizeof(temp) || inlen > rsiz || rsiz - inlen + inlen + 1 >= sizeof(temp) || rsiz == 0 || rsiz - 1 >= sizeof(temp) || rsizw * 8 > sizeof(temp))
    {
      local_abort("Bad keccak use");
    }

    if (inlen > 0)
      memcpy(temp, in, inlen);
    temp[inlen++] = 1;
    memset(temp + inlen, 0, rsiz - inlen);
    temp[rsiz - 1] |= 0x80;

    for (i = 0; i < rsizw; i++)
        st[i] ^= swap64le(((uint64_t *) temp)[i]);

    keccakf(st, KECCAK_ROUNDS);

    if (((size_t)mdlen % sizeof(uint64_t)) != 0)
    {
      local_abort("Bad keccak use");
    }
    memcpy_swap64le(md, st, mdlen/sizeof(uint64_t));
}

void sc_reduce32(unsigned char *s) {
  int64_t s0 = 2097151 & load_3(s);
  int64_t s1 = 2097151 & (load_4(s + 2) >> 5);
  int64_t s2 = 2097151 & (load_3(s + 5) >> 2);
  int64_t s3 = 2097151 & (load_4(s + 7) >> 7);
  int64_t s4 = 2097151 & (load_4(s + 10) >> 4);
  int64_t s5 = 2097151 & (load_3(s + 13) >> 1);
  int64_t s6 = 2097151 & (load_4(s + 15) >> 6);
  int64_t s7 = 2097151 & (load_3(s + 18) >> 3);
  int64_t s8 = 2097151 & load_3(s + 21);
  int64_t s9 = 2097151 & (load_4(s + 23) >> 5);
  int64_t s10 = 2097151 & (load_3(s + 26) >> 2);
  int64_t s11 = (load_4(s + 28) >> 7);
  int64_t s12 = 0;
  int64_t carry0;
  int64_t carry1;
  int64_t carry2;
  int64_t carry3;
  int64_t carry4;
  int64_t carry5;
  int64_t carry6;
  int64_t carry7;
  int64_t carry8;
  int64_t carry9;
  int64_t carry10;
  int64_t carry11;

  carry0 = (s0 + (1<<20)) >> 21; s1 += carry0; s0 -= carry0 << 21;
  carry2 = (s2 + (1<<20)) >> 21; s3 += carry2; s2 -= carry2 << 21;
  carry4 = (s4 + (1<<20)) >> 21; s5 += carry4; s4 -= carry4 << 21;
  carry6 = (s6 + (1<<20)) >> 21; s7 += carry6; s6 -= carry6 << 21;
  carry8 = (s8 + (1<<20)) >> 21; s9 += carry8; s8 -= carry8 << 21;
  carry10 = (s10 + (1<<20)) >> 21; s11 += carry10; s10 -= carry10 << 21;

  carry1 = (s1 + (1<<20)) >> 21; s2 += carry1; s1 -= carry1 << 21;
  carry3 = (s3 + (1<<20)) >> 21; s4 += carry3; s3 -= carry3 << 21;
  carry5 = (s5 + (1<<20)) >> 21; s6 += carry5; s5 -= carry5 << 21;
  carry7 = (s7 + (1<<20)) >> 21; s8 += carry7; s7 -= carry7 << 21;
  carry9 = (s9 + (1<<20)) >> 21; s10 += carry9; s9 -= carry9 << 21;
  carry11 = (s11 + (1<<20)) >> 21; s12 += carry11; s11 -= carry11 << 21;

  s0 += s12 * 666643;
  s1 += s12 * 470296;
  s2 += s12 * 654183;
  s3 -= s12 * 997805;
  s4 += s12 * 136657;
  s5 -= s12 * 683901;
  s12 = 0;

  carry0 = s0 >> 21; s1 += carry0; s0 -= carry0 << 21;
  carry1 = s1 >> 21; s2 += carry1; s1 -= carry1 << 21;
  carry2 = s2 >> 21; s3 += carry2; s2 -= carry2 << 21;
  carry3 = s3 >> 21; s4 += carry3; s3 -= carry3 << 21;
  carry4 = s4 >> 21; s5 += carry4; s4 -= carry4 << 21;
  carry5 = s5 >> 21; s6 += carry5; s5 -= carry5 << 21;
  carry6 = s6 >> 21; s7 += carry6; s6 -= carry6 << 21;
  carry7 = s7 >> 21; s8 += carry7; s7 -= carry7 << 21;
  carry8 = s8 >> 21; s9 += carry8; s8 -= carry8 << 21;
  carry9 = s9 >> 21; s10 += carry9; s9 -= carry9 << 21;
  carry10 = s10 >> 21; s11 += carry10; s10 -= carry10 << 21;
  carry11 = s11 >> 21; s12 += carry11; s11 -= carry11 << 21;

  s0 += s12 * 666643;
  s1 += s12 * 470296;
  s2 += s12 * 654183;
  s3 -= s12 * 997805;
  s4 += s12 * 136657;
  s5 -= s12 * 683901;

  carry0 = s0 >> 21; s1 += carry0; s0 -= carry0 << 21;
  carry1 = s1 >> 21; s2 += carry1; s1 -= carry1 << 21;
  carry2 = s2 >> 21; s3 += carry2; s2 -= carry2 << 21;
  carry3 = s3 >> 21; s4 += carry3; s3 -= carry3 << 21;
  carry4 = s4 >> 21; s5 += carry4; s4 -= carry4 << 21;
  carry5 = s5 >> 21; s6 += carry5; s5 -= carry5 << 21;
  carry6 = s6 >> 21; s7 += carry6; s6 -= carry6 << 21;
  carry7 = s7 >> 21; s8 += carry7; s7 -= carry7 << 21;
  carry8 = s8 >> 21; s9 += carry8; s8 -= carry8 << 21;
  carry9 = s9 >> 21; s10 += carry9; s9 -= carry9 << 21;
  carry10 = s10 >> 21; s11 += carry10; s10 -= carry10 << 21;

  s[0] = s0 >> 0;
  s[1] = s0 >> 8;
  s[2] = (s0 >> 16) | (s1 << 5);
  s[3] = s1 >> 3;
  s[4] = s1 >> 11;
  s[5] = (s1 >> 19) | (s2 << 2);
  s[6] = s2 >> 6;
  s[7] = (s2 >> 14) | (s3 << 7);
  s[8] = s3 >> 1;
  s[9] = s3 >> 9;
  s[10] = (s3 >> 17) | (s4 << 4);
  s[11] = s4 >> 4;
  s[12] = s4 >> 12;
  s[13] = (s4 >> 20) | (s5 << 1);
  s[14] = s5 >> 7;
  s[15] = (s5 >> 15) | (s6 << 6);
  s[16] = s6 >> 2;
  s[17] = s6 >> 10;
  s[18] = (s6 >> 18) | (s7 << 3);
  s[19] = s7 >> 5;
  s[20] = s7 >> 13;
  s[21] = s8 >> 0;
  s[22] = s8 >> 8;
  s[23] = (s8 >> 16) | (s9 << 5);
  s[24] = s9 >> 3;
  s[25] = s9 >> 11;
  s[26] = (s9 >> 19) | (s10 << 2);
  s[27] = s10 >> 6;
  s[28] = (s10 >> 14) | (s11 << 7);
  s[29] = s11 >> 1;
  s[30] = s11 >> 9;
  s[31] = s11 >> 17;
}
