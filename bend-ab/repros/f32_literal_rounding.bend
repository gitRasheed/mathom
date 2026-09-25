# An F32 literal is not rounded to the nearest f32: the front end parses it
# as a double, then rounds that double to f32 (bend2/bend.ts: Math.fround(
# Number(..))). A decimal just past an f32 midpoint collapses onto the
# midpoint as a double, and ties-to-even then rounds it the wrong way.
#
# 16777217.00000000001 lies above the midpoint of 16777216 and 16777218:
#   correct (Rust "..".parse::<f32>(), C strtof): 16777218, bits 1266679809
#   Bend, every lane (C, JS, checker):            16777216, bits 1266679808
import Base

def main() -> IO(Unit):
  do IO<Unit>:
    IO.print(U32.show(F32.bits(16777217.00000000001)))
    IO.print(U32.show(F32.bits(1.0000000596046447753906250001)))

# C, JS print 1266679808 and 1065353216; correct is 1266679809 and 1065353217
