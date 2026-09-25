// Enumerates every finite f32 whose exact value is a decimal tie at nine
// or fewer significant digits, i.e. 2x = odd * 10^k with (odd-1)/2 < 10^9:
// these are the only values on which printf's half-even and
// toExponential's half-up can print different digits. Walks all 2^32 bit
// patterns; prints the candidate bits, one per line, both signs.
//   tiecands            every candidate
//   tiecands sample N   instead: N random mantissas per exponent and sign
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

int main(int argc, char** argv) {
  if (argc > 2 && strcmp(argv[1], "sample") == 0) {
    int n = atoi(argv[2]);
    srand(20260925);
    for (uint32_t s = 0; s < 2; s++) {
      for (uint32_t e = 0; e < 255; e++) {
        for (int i = 0; i < n; i++) {
          uint32_t m = ((uint32_t)rand() ^ ((uint32_t)rand() << 12)) & 0x7fffff;
          printf("%u\n", (s << 31) | (e << 23) | m);
        }
      }
    }
    return 0;
  }
  uint64_t count = 0;
  for (uint64_t bits = 0; bits < 0x80000000ull; bits++) {
    uint32_t e = (bits >> 23) & 0xff;
    uint32_t f = bits & 0x7fffff;
    if (e == 255 || (e == 0 && f == 0)) {
      continue;
    }
    uint64_t m = e == 0 ? f : f | 0x800000;
    int p = (e == 0 ? 1 : (int)e) - 150;
    while ((m & 1) == 0) {
      m >>= 1;
      p += 1;
    }
    int tie = 0;
    if (p < 0) {
      int j = -p - 1;
      if (j <= 13) {
        uint64_t odd = m;
        for (int i = 0; i < j; i++) {
          odd *= 5;
        }
        tie = (odd - 1) / 2 < 1000000000ull;
      }
    } else {
      uint64_t d = 1;
      for (int i = 0; i <= p; i++) {
        d *= 5;
        if (d > m) {
          break;
        }
      }
      tie = p <= 10 && m % d == 0;
    }
    if (tie) {
      printf("%u\n%u\n", (uint32_t)bits, (uint32_t)(bits | 0x80000000u));
      count += 2;
    }
  }
  fprintf(stderr, "%llu candidates\n", (unsigned long long)count);
  return 0;
}
