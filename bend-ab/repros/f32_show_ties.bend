# F32.show picks a different shortest digit string on each lane when two
# round-trip: C's printf rounds a decimal tie to even, JS's toExponential
# rounds it up. 104 of 200,000 fuzzed floats differ. `bend file.bend` runs
# IO through the JS path, so it disagrees with the compiled binary too.
#   C (./bin):              23577.562  2335232.2  -2983783.2
#   JS (bun, bend file.bend): 23577.563  2335232.3  -2983783.3
import Base

def main() -> IO(Unit):
  do IO<Unit>:
    IO.print(F32.show(23577.5625))
    IO.print(F32.show(2335232.25))
    IO.print(F32.show(F32.neg(2983783.25)))
