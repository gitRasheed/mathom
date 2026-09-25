"""Decode the string literal term_show prints for a pure String main."""
import re, sys
raw = open(sys.argv[1], encoding='utf-8').read().strip()
if not raw.startswith('"'):
    open(sys.argv[2], 'w', encoding='utf-8').write('NOT A STRING: ' + raw[:2000])
    sys.exit(0)
body = raw[1:-1]
ESC = {'n': '\n', 't': '\t', 'r': '\r', '0': '\0', '\\': '\\', '"': '"', "'": "'"}
out, i = [], 0
while i < len(body):
    c = body[i]
    if c == '\\':
        n = body[i + 1]
        if n == 'u':
            j = body.index('}', i)
            out.append(chr(int(body[i + 3:j], 16)))
            i = j + 1
            continue
        out.append(ESC[n])
        i += 2
        continue
    out.append(c)
    i += 1
open(sys.argv[2], 'w', encoding='utf-8').write(''.join(out))
