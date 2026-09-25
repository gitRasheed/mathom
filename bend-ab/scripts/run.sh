#!/usr/bin/env bash
# A/B harness: mathom's Rust code is the oracle, the Bend ports under ports/
# must reproduce its output on every Bend lane.
#
# usage: BEND_ROOT=/path/to/Bend scripts/run.sh <suite> [args]
#   runs [N]              NTFS data-run decoding, N generated run lists
#   category [N]          extension keys and categories, N generated names
#   tree SEED DEPTH       tree aggregates, type breakdown, top files, search,
#                         overlay (DEPTH 14 is ~70k nodes)
#   treemap SEED DEPTH    squarified layout in F32, bit-exact against an f32
#                         mirror of treemap.rs (DEPTH 10 is ~2k rects)
#   ops                   U32/Nat primitives and U32.read/Nat.read, C vs JS
#   interp SUITE N CHUNK  runs|category through the checker's normalizer
#                         (the third lane; slow, so N small)
#
# Lanes: the native C binary (at several --threads counts where it matters)
# and the JS build under bun. Needs bun, clang and cargo on PATH.
set -uo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
: "${BEND_ROOT:?set BEND_ROOT to a Bend checkout}"
BEND=(bun "$BEND_ROOT/bend2/main.ts")
ORACLE=$HERE/oracle/target/release/oracle
THREADS=${THREADS:-"1 4 16"}

(cd "$HERE/oracle" && cargo build --release -q) || exit 1

now() { date +%s.%N; }
since() { echo "$(now) - $1" | bc; }

# diff_count <expected> <got>: lines that differ
diff_count() { diff "$1" "$2" | grep -c '^[<>]'; }

# build <dir> <main.bend>: out/main (C) and out/main.js
build() {
  local dir=$1 main=$2 t
  mkdir -p "$dir/out"
  t=$(now)
  "${BEND[@]}" "$dir/$main" -o "$dir/out/main" > "$dir/out/build_c.log" 2>&1 \
    || { echo "C build failed:"; head -20 "$dir/out/build_c.log"; }
  "${BEND[@]}" "$dir/$main" -o "$dir/out/main.js" > "$dir/out/build_js.log" 2>&1 \
    || { echo "JS build failed:"; head -20 "$dir/out/build_js.log"; }
  echo "build: $(since "$t")s"
}

# lanes <dir> <expected> <threads...>
lanes() {
  local dir=$1 want=$2 t rc
  shift 2
  for th in "$@"; do
    t=$(now)
    timeout 900 "$dir/out/main" --threads "$th" > "$dir/out/c.txt" 2> "$dir/out/c.err"
    rc=$?
    echo "c   threads=$th rc=$rc time=$(since "$t")s mismatched=$(diff_count "$want" "$dir/out/c.txt") $(head -c 160 "$dir/out/c.err")"
  done
  t=$(now)
  timeout 900 bun "$dir/out/main.js" > "$dir/out/js.txt" 2> "$dir/out/js.err"
  rc=$?
  echo "js  rc=$rc time=$(since "$t")s mismatched=$(diff_count "$want" "$dir/out/js.txt") $(head -c 160 "$dir/out/js.err")"
}

# list_suite <oracle suite> <dir> <port> <cases def> <n>
list_suite() {
  local suite=$1 dir=$2 port=$3 cases=$4 n=$5
  "$ORACLE" "$suite" "$n" "$dir/gen"
  cat > "$dir/main.bend" <<EOF
import Base
import ./$port as P
import ./gen/cases.bend as C

def main() -> IO(Unit):
  P.run(C.$cases())
EOF
  build "$dir" main.bend
  lanes "$dir" "$dir/gen/expected.txt" 1
}

# interp_suite <oracle suite> <dir> <port> <cases def> <n> <chunk>
interp_suite() {
  local suite=$1 dir=$2 port=$3 cases=$4 n=$5 chunk=$6 k=0 t
  BEND_CHUNK=$chunk "$ORACLE" "$suite" "$n" "$dir/gen_small"
  mkdir -p "$dir/out"
  : > "$dir/out/interp.txt"
  t=$(now)
  while grep -q "^def $cases.c$k()" "$dir/gen_small/cases.bend"; do
    cat > "$dir/pure.bend" <<EOF
import Base
import ./$port as P
import ./gen_small/cases.bend as C

def main() -> String:
  P.lines.one(C.$cases.c$k())
EOF
    "${BEND[@]}" "$dir/pure.bend" > "$dir/out/interp.raw" 2>> "$dir/out/interp.err"
    python3 "$HERE/scripts/decode.py" "$dir/out/interp.raw" "$dir/out/interp.part"
    cat "$dir/out/interp.part" >> "$dir/out/interp.txt"
    k=$((k + 1))
  done
  echo "interp chunks=$k time=$(since "$t")s mismatched=$(diff_count "$dir/gen_small/expected.txt" "$dir/out/interp.txt")"
}

case "${1:-}" in
  runs)
    list_suite runs "$HERE/ports/runs" runs.bend cases "${2:-3000}" ;;
  category)
    list_suite cat "$HERE/ports/category" cat.bend names "${2:-3000}" ;;
  interp)
    case "$2" in
      runs) interp_suite runs "$HERE/ports/runs" runs.bend cases "$3" "$4" ;;
      category) interp_suite cat "$HERE/ports/category" cat.bend names "$3" "$4" ;;
      *) echo "interp: runs|category" >&2; exit 2 ;;
    esac ;;
  tree)
    dir=$HERE/ports/tree
    "$ORACLE" tree "$2" "$dir/gen" "$3"
    head -1 "$dir/gen/expected.txt"
    printf 'import Base\nimport ./tree.bend as T\n\ndef main() -> IO(Unit):\n  T.main_io()\n' > "$dir/main.bend"
    build "$dir" main.bend
    # shellcheck disable=SC2086
    lanes "$dir" "$dir/gen/expected.txt" $THREADS ;;
  treemap)
    dir=$HERE/ports/tree
    "$ORACLE" treemap "$2" "$dir/gen" "$3"
    echo "rects $(wc -l < "$dir/gen/expected.txt")"
    printf 'import Base\nimport ./treemap.bend as M\n\ndef main() -> IO(Unit):\n  IO.write(M.layout())\n' > "$dir/main.bend"
    build "$dir" main.bend
    # shellcheck disable=SC2086
    lanes "$dir" "$dir/gen/expected.txt" $THREADS ;;
  ops)
    dir=$HERE/ports/ops
    printf 'import Base\nimport ./ops.bend as O\n\ndef main() -> IO(Unit):\n  O.all()\n' > "$dir/main.bend"
    build "$dir" main.bend
    "$dir/out/main" > "$dir/out/c.txt"
    bun "$dir/out/main.js" > "$dir/out/js.txt"
    echo "ops lines=$(wc -l < "$dir/out/c.txt") c-vs-js mismatched=$(diff_count "$dir/out/c.txt" "$dir/out/js.txt")" ;;
  *)
    sed -n '2,20p' "$0"; exit 2 ;;
esac
