# A pure main is normalized by the checker and printed; a String past
# roughly 6,000-10,000 characters overflows the machine stack there, so
# the checker lane cannot print a result the compiled lanes print fine.
#   bend normalizer_long_string.bend: "Error: the machine stack overflowed"
import Base

def main() -> String:
  String.repeat("a", 10000n)
