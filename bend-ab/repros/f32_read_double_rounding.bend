# F32.read rounds twice on the JS lane (Number(s), then Math.fround; see
# f32_read in bend2/comp.ts) but once on the C lane (strtof). The lanes
# disagree, and the C lane is the correctly rounded one.
#   C:  1266679809 then 1065353217
#   JS: 1266679808 then 1065353216
import Base

def bits(m: Maybe<&2, F32>) -> String:
  match m:
    case None{}:
      "None"
    case Some{v}:
      U32.show(F32.bits(v))

def main() -> IO(Unit):
  do IO<Unit>:
    IO.print(bits(F32.read("16777217.00000000001")))
    IO.print(bits(F32.read("1.0000000596046447753906250001")))
