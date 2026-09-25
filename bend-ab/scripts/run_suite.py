"""Local replica of Bend's gates/test.ts, for a Linux x86-64 host.

Per test: the check lane (`bend t.bend`, which checks and runs main), then,
for runnable tests, one build with `-o t.js -o t` and a C run and a JS run.
Output is stdout+stderr, tidied as the gate tidies it, with "exit N" or
"timeout" appended. Results go to results.jsonl, one line per test.
"""
import json, os, re, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

BEND_ROOT = sys.argv[1]
OUT = sys.argv[2]
JOBS = int(sys.argv[3]) if len(sys.argv) > 3 else 3
ONLY = sys.argv[4] if len(sys.argv) > 4 else None
TESTS = os.path.join(BEND_ROOT, "tests")
MAIN = os.path.join(BEND_ROOT, "bend2", "main.ts")
ENV = dict(os.environ, BUN_JSC_maxPerThreadStackUsage="33554432")
RUN_TIMEOUT = 20  # the gate's 5 s is tuned for an M4


def tidy(t):
    return re.sub(r"[ \t]+$", "", t, flags=re.M).strip()


def read(ns, f):
    src = open(os.path.join(TESTS, ns, f), encoding="utf-8").read()
    want = "\n".join(l[2:] for l in src.split("\n") if l.startswith("#|"))
    effs = re.findall(r'^\s*import "\./[a-z0-9_]+\.(c|js)"$', src, flags=re.M)
    has_base = re.search(r"^import Base$", src, flags=re.M) is not None
    lanes = [l for l in ("js", "c") if has_base and (not effs or l in effs)]
    return {
        "name": ns + "_" + f[:-5], "path": os.path.join(TESTS, ns, f),
        "want": tidy(want), "main": re.search(r"^(def|law) main(\(|:)", src, flags=re.M) is not None,
        "lanes": lanes, "bang": "!(" in src,
    }


def run(cmd, timeout, cwd=None):
    try:
        p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                           timeout=timeout, env=ENV, cwd=cwd)
        out = tidy(p.stdout.decode("utf-8", "replace"))
        if p.returncode != 0:
            out = (out + "\n" if out else "") + "exit " + str(p.returncode)
        return out
    except subprocess.TimeoutExpired:
        return "timeout"


def one(t):
    r = {"name": t["name"], "want": t["want"], "bang": t["bang"], "got": {}}
    wd = os.path.dirname(t["path"])
    # check lane: the gate interprets through --checkup, which runs main the
    # way `bend t.bend` does
    agg = os.path.join(OUT, t["name"] + ".agg.bend")
    open(agg, "w").write("import " + t["path"] + " as T\n")
    chk = run(["bun", MAIN, agg, "--checkup"], 300, cwd=wd)
    chk = re.sub(r"^--- .* ---\n?", "", chk)
    chk = re.sub(r"\nexit 1\nexit 1$", "\nexit 1", chk)
    r["got"]["check"] = tidy(chk)
    runnable = t["main"] and t["lanes"] and not t["want"].startswith("Error:")
    if runnable:
        base = os.path.join(OUT, t["name"])
        outs = []
        for l in t["lanes"]:
            outs += ["-o", base + (".js" if l == "js" else "")]
        b = run(["bun", MAIN, t["path"]] + outs, 600, cwd=wd)
        if b and b != "":
            r["build"] = b
        for l in t["lanes"]:
            if l == "c":
                exe = base
                r["got"]["c"] = run([exe], RUN_TIMEOUT, cwd=wd) if os.path.exists(exe) else "(no binary) " + b
            else:
                js = base + ".js"
                r["got"]["js"] = run(["bun", js], RUN_TIMEOUT, cwd=wd) if os.path.exists(js) else "(no js) " + b
    fails = []
    for lane, got in r["got"].items():
        if lane == "check":
            if t["main"] and not t["want"].startswith("Error:"):
                # the gate also compares the interpreted run to the want
                ok = got == t["want"] or (not t["lanes"] and not got.startswith("Error:") and got == t["want"])
                ok = got == t["want"]
            else:
                ok = got == t["want"] if t["want"].startswith("Error:") else not got.startswith("Error:")
        else:
            ok = got == t["want"]
        if not ok:
            fails.append(lane)
    r["fails"] = fails
    return r


def main():
    os.makedirs(OUT, exist_ok=True)
    tests = []
    for ns in sorted(os.listdir(TESTS)):
        d = os.path.join(TESTS, ns)
        if not os.path.isdir(d):
            continue
        for f in sorted(os.listdir(d)):
            if f.endswith(".bend") and (ONLY is None or re.search(ONLY, ns + "_" + f)):
                tests.append(read(ns, f))
    done = set()
    res_path = os.path.join(OUT, "results.jsonl")
    if os.path.exists(res_path):
        for l in open(res_path):
            done.add(json.loads(l)["name"])
    todo = [t for t in tests if t["name"] not in done]
    print(f"{len(tests)} tests, {len(todo)} to run", flush=True)
    with open(res_path, "a") as fo, ThreadPoolExecutor(JOBS) as ex:
        for i, r in enumerate(ex.map(one, todo)):
            fo.write(json.dumps(r) + "\n")
            fo.flush()
            if r["fails"]:
                print("FAIL", r["name"], r["fails"], flush=True)
            if i % 50 == 0:
                print(f"[{i}/{len(todo)}]", flush=True)


main()
