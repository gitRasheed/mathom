#!/bin/sh
# Pulls the JS runtime helpers (word_to_u32 .. char_new) verbatim out of a
# comp.ts, so the harness tests the exact text the compiler emits.
#   extract_js.sh <comp.ts> > funcs.js
awk '/^function word_to_u32/{on=1} on && /^`\.slice\(1\),/{exit} on' "$1"
