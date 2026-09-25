# Overdue sleepers resume in the order they parked, not in deadline order,
# on both lanes (io_wait in bend2/comp.ts, C and JS). Here b's 1 ms
# deadline passes before a's 9 ms one, but a third computation keeps the
# loop busy past both, so the loop finds both due at once and resumes a
# first.
#   ./late_wake_order x        C:  busy 6000000, a, b   (want b, a)
# The JS lane shows the same thing on Bend's own tests/io/spawn_sleep.bend:
# on Linux its first io_wait lazily dlopens libc through bun:ffi (about
# 14 ms) after computing a 1 ms timeout, so it wakes late on every run.
# (This file's busy work overflows the JS stack; see WONTFIX #798.)
import Base

def ping(ms: U32, tag: String) -> IO(Unit):
  do IO<Unit>:
    IO.sleep(ms)
    IO.print(tag)

# The argument count scales the work so the compiler cannot fold it.
def busy(n: Nat) -> IO(Unit):
  do IO<Unit>:
    args : List<String> <- IO.args()
    IO.print("busy " ++ Nat.show(String.length(String.repeat("ab",
      Nat.mul(n, List.length(&1, String, args))))))

def main() -> IO(Unit):
  do IO<Unit>:
    IO.spawn(Unit, ping(9, "a"))
    IO.spawn(Unit, ping(1, "b"))
    IO.spawn(Unit, busy(3000000n))
