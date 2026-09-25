// The C lane's F32.read and F32.show, as bend2/comp.ts defines them
// (f32_text copied verbatim; f32_read's whole-string gate and its
// "xX(" refusal reproduced), driven line by line from stdin.
//   oracle read  : each line is a text; prints the bits of Some, or None
//   oracle show  : each line is a u32 (decimal bits); prints F32.show's text
// A NaN prints as "nan" in read mode (payload bits are WONTFIX #797).
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

typedef float f32;

static int f32_text(char* buf, f32 v) {
  int n = 0;
  int p = 0;
  if (v != v) {
    return sprintf(buf, "nan");
  }
  for (; p < 9; p += 1) {
    n = snprintf(buf, 40, "%.*e", p, (double)v);
    if (strtof(buf, NULL) == v) {
      break;
    }
  }
  char* ep = strchr(buf, 'e');
  if (ep == NULL) {
    return n;
  }
  int ex = atoi(ep + 1);
  if (ex >= 21 || ex <= -7) {
    n = (int)(ep - buf) + sprintf(ep, "e%c%d", ex < 0 ? '-' : '+', abs(ex));
  } else if (ex <= p) {
    n = snprintf(buf, 40, "%.*f", p - ex, (double)v);
  } else {
    int s = *buf == '-';
    memmove(buf + s + 1, buf + s + 2, p);
    memset(buf + s + 1 + p, '0', ex - p);
    n = s + 1 + ex;
  }
  return n;
}

int main(int argc, char** argv) {
  static char line[1 << 20];
  int show = argc > 1 && strcmp(argv[1], "show") == 0;
  while (fgets(line, sizeof line, stdin)) {
    size_t n = strcspn(line, "\n");
    line[n] = 0;
    if (show) {
      uint32_t u = (uint32_t)strtoul(line, NULL, 10);
      f32 v;
      memcpy(&v, &u, 4);
      char buf[40];
      int k = f32_text(buf, v);
      buf[k] = 0;
      puts(buf);
    } else {
      char* end;
      f32 v = strtof(line, &end);
      if (n > 0 && (size_t)(end - line) == n && strpbrk(line, "xX(") == NULL) {
        if (v != v) {
          puts("nan");
        } else {
          uint32_t u;
          memcpy(&u, &v, 4);
          printf("%u\n", u);
        }
      } else {
        puts("None");
      }
    }
  }
  return 0;
}
