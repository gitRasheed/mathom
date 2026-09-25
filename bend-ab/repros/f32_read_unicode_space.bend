# F32.read accepts leading Unicode whitespace on the JS lane only: the JS
# regex's \s and Number() both take NBSP, EM SPACE and BOM; C's strtof
# skips ASCII whitespace only.
#   C:  None None None
#   JS: Some Some Some
import Base

def show(m: Maybe<&2, F32>) -> String:
  match m:
    case None{}:
      "None"
    case Some{v}:
      "Some"

def main() -> IO(Unit):
  do IO<Unit>:
    IO.print(show(F32.read("\u{a0}1")))
    IO.print(show(F32.read("\u{2003}2.5")))
    IO.print(show(F32.read("\u{feff}3")))
