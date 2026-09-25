# U32 / Nat primitive semantics: the compiled intrinsics vs the theory
import Base

def V() -> List<&2, U32>:
  [0, 1, 2, 3, 7, 8, 31, 32, 33, 255, 256, 65535, 65536, 2147483647,
   2147483648, 2147483649, 4294967294, 4294967295, 3735928559, 123456789]

def cmp.show(c: Cmp) -> String:
  match c:
    case LT{}:
      "LT"
    case EQ{}:
      "EQ"
    case GT{}:
      "GT"

def bool.show(b: Bool) -> String:
  match b:
    case True{}:
      "T"
    case False{}:
      "F"

def pair(+a: U32, +b: U32) -> String:
  +s = U32.to_nat(U32.mod(b, 40))
  U32.show(a) ++ " " ++ U32.show(b)
    ++ " add " ++ U32.show(U32.add(a, b)) ++ " sub " ++ U32.show(U32.sub(a, b))
    ++ " mul " ++ U32.show(U32.mul(a, b)) ++ " div " ++ U32.show(U32.div(a, b))
    ++ " mod " ++ U32.show(U32.mod(a, b)) ++ " and " ++ U32.show((a .&. b : U32))
    ++ " or " ++ U32.show((a .|. b : U32)) ++ " xor " ++ U32.show((a .^. b : U32))
    ++ " shl " ++ U32.show(U32.shln(a, s)) ++ " shr " ++ U32.show(U32.shrn(a, s))
    ++ " cmp " ++ cmp.show(U32.cmp(a, b)) ++ " lt " ++ bool.show(U32.is_lt(a, b))
    ++ " le " ++ bool.show(U32.is_le(a, b)) ++ " eq " ++ bool.show(U32.is_eq(a, b))
    ++ " not " ++ U32.show(U32.not(a)) ++ " inc " ++ U32.show(U32.inc(a))
    ++ " min " ++ U32.show(U32.min(a, b)) ++ " max " ++ U32.show(U32.max(a, b))
    ++ " nsub " ++ Nat.show(Nat.sub(U32.to_nat(a), U32.to_nat(b)))
    ++ " ndiv " ++ Nat.show(Nat.div(U32.to_nat(a), U32.to_nat(b)))
    ++ " nmod " ++ Nat.show(Nat.mod(U32.to_nat(a), U32.to_nat(b)))
    ++ " nmul " ++ Nat.show(Nat.mul(U32.to_nat(U32.mod(a, 65536)), U32.to_nat(b)))
    ++ " fromnat " ++ U32.show(U32.from_nat(Nat.add(U32.to_nat(a), U32.to_nat(b))))
    ++ " log2 " ++ Nat.show(U32.log2(a)) ++ " pow " ++ U32.show(U32.pow(a, U32.to_nat(U32.mod(b, 5))))
    ++ "\n"

def row(+a: U32, bs: List<&2, U32>) -> String:
  match bs:
    case Nil{}:
      ""
    case b <> t:
      pair(a, b) ++ row(a, t)

def rows(as: List<&2, U32>) -> String:
  match as:
    case Nil{}:
      ""
    case a <> t:
      row(a, V()) ++ rows(t)

def maybe.u32(m: Maybe<&2, U32>) -> String:
  match m:
    case None{}:
      "None"
    case Some{v}:
      "Some " ++ U32.show(v)

def maybe.nat(m: Maybe<&2, Nat>) -> String:
  match m:
    case None{}:
      "None"
    case Some{v}:
      "Some " ++ Nat.show(v)

def S() -> List<&2, String>:
  ["0", "00", "7", "4294967295", "4294967296", "99999999999", "-1", "+1", " 1",
   "1 ", "", "1a", "281474976710655", "281474976710656", "999999999999999",
   "1000000000000000", "0000000000000000001", "\u{0661}", "12\u{0}3"]

def reads(xs: List<&2, String>) -> String:
  match xs:
    case Nil{}:
      ""
    case +h <> t:
      "read [" ++ h ++ "] u32 " ++ maybe.u32(U32.read(h)) ++ " nat "
        ++ maybe.nat(Nat.read(h)) ++ "\n" ++ reads(t)

def nth(xs: List<&2, U32>, n: Nat) -> List<&2, U32>:
  match xs:
    case Nil{}:
      Nil{}
    case h <> t:
      match n:
        case 0n:
          [h]
        case 1n+p:
          nth(t, p)

# one row block per call, so the normalizer lane stays under its output cap
def block.if(rowsk: Bool, +k: Nat) -> String:
  match rowsk:
    case True{}:
      rows(nth(V(), k))
    case False{}:
      reads(S())

def block(+k: Nat) -> String:
  block.if(Nat.is_lt(k, 20n), k)

def all.go(n: Nat, +k: Nat) -> IO(Unit):
  match n:
    case 0n:
      IO.pure(Unit, Unit{})
    case 1n+p:
      do IO<Unit>:
        IO.write(block(k))
        all.go(p, 1n+k)

def all() -> IO(Unit):
  all.go(21n, 0n)

def pair.lite(+a: U32, +b: U32) -> String:
  +s = U32.to_nat(U32.mod(b, 40))
  U32.show(a) ++ " " ++ U32.show(b)
    ++ " add " ++ U32.show(U32.add(a, b)) ++ " sub " ++ U32.show(U32.sub(a, b))
    ++ " mul " ++ U32.show(U32.mul(a, b)) ++ " div " ++ U32.show(U32.div(a, b))
    ++ " mod " ++ U32.show(U32.mod(a, b)) ++ " and " ++ U32.show((a .&. b : U32))
    ++ " or " ++ U32.show((a .|. b : U32)) ++ " xor " ++ U32.show((a .^. b : U32))
    ++ " shl " ++ U32.show(U32.shln(a, s)) ++ " shr " ++ U32.show(U32.shrn(a, s))
    ++ " cmp " ++ cmp.show(U32.cmp(a, b)) ++ " lt " ++ bool.show(U32.is_lt(a, b))
    ++ " le " ++ bool.show(U32.is_le(a, b)) ++ " eq " ++ bool.show(U32.is_eq(a, b))
    ++ " not " ++ U32.show(U32.not(a)) ++ " inc " ++ U32.show(U32.inc(a))
    ++ " min " ++ U32.show(U32.min(a, b)) ++ " max " ++ U32.show(U32.max(a, b))
    ++ "\n"

def row.lite(+a: U32, bs: List<&2, U32>) -> String:
  match bs:
    case Nil{}:
      ""
    case b <> t:
      pair.lite(a, b) ++ row.lite(a, t)

def block.lite.go(xs: List<&2, U32>) -> String:
  match xs:
    case Nil{}:
      ""
    case a <> t:
      row.lite(a, V())

def block.lite(+k: Nat) -> String:
  block.lite.go(nth(V(), k))
