"""Summarize run_suite.py results the way gates/test.ts judges them.

Two things the gate does that run_suite.py records raw: it skips the C and
JS lanes of a test whose main the compiler refuses to print, and it reads
the checkup section only, not the exit code of the checkup process itself.
"""
import json, re, sys

rows = [json.loads(l) for l in open(sys.argv[1])]
real = []
for r in rows:
    fails = []
    for lane in r["fails"]:
        got = r["got"].get(lane, "")
        if lane in ("c", "js") and "cannot be printed" in r.get("build", ""):
            continue
        if lane == "check" and re.sub(r"\nexit 1$", "", got) == r["want"]:
            continue
        fails.append(lane)
    if fails:
        real.append((r, fails))
lanes = {k: sum(1 for r in rows if k in r["got"]) for k in ("check", "c", "js")}
print(f"tests {len(rows)}; lanes run: check {lanes['check']}, c {lanes['c']}, js {lanes['js']}")
print(f"failing after the gate's own exclusions: {len(real)}")
for r, fails in real:
    print(f"== {r['name']} {fails}{' (uses !)' if r['bang'] else ''}")
    print("   want:", r["want"][:300].replace("\n", "|"))
    if "build" in r:
        print("   build:", r["build"][:300].replace("\n", "|"))
    for k in fails:
        print(f"   {k}:", r["got"][k][:300].replace("\n", "|"))
