# A Char is Chr{code: U32} in the theory, and the checker and the C lane
# accept any code. The JS lane throws on a surrogate or a code past
# U+10FFFF (char_new in bend2/comp.ts), and the whole run dies.
#   checker: evaluates fine
#   C:       prints the surrogate as WTF-8 (ED A0 80); 0x110000 as F4 90 80 80
#   JS:      "bend: 55296 is not a Unicode scalar value", exit 1
# A lone surrogate is an ordinary NTFS file name, so a Bend port of a
# Windows scanner that decodes UTF-16 names meets this.
import Base

def main() -> IO(Unit):
  do IO<Unit>:
    IO.print(Nat.show(String.length(SCon{Chr{55296}, SNil{}})))
    IO.print(SCon{Chr{55296}, SNil{}})
    IO.print(SCon{Chr{1114112}, SNil{}})
