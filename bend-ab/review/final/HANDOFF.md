# Handoff: apply the F32 fix in a fork of bendlang/bend

Paste the prompt below into a Claude session attached to your fork.

```
In this repo (my fork of bendlang/bend), make a branch `f32-read-show-lanes` from the latest upstream main.

Get the patch from my mathom repo, branch `claude/exciting-shannon-feotrp`, file `bend-ab/review/fix.patch`:
https://raw.githubusercontent.com/gitRasheed/mathom/claude/exciting-shannon-feotrp/bend-ab/review/fix.patch
(if that URL is not reachable, clone gitRasheed/mathom at that branch and copy the file).

Apply it with `git apply --3way`. It changes bend2/comp.ts (the JS runtime's f32_read and f32_show, plus a dec_cmp helper), adds tests/run/float_text_lanes.bend and edits one comment line in tests/run/float_text.bend. If upstream has since merged the F32.fma (#1046) or F32.pow (#1062) changes, they insert helpers at the same spot in comp.ts: keep both sides.

Check it (needs bun and clang):
  bun bend2/main.ts tests/run/float_text_lanes.bend
  bun bend2/main.ts tests/run/float_text_lanes.bend -o /tmp/t -o /tmp/t.js && /tmp/t && bun /tmp/t.js
All three must print exactly:
  1266679809 2139095039 2147483649 None 1065353216 23577.562 -2983783.2
Also run tests/run/float_text.bend the same three ways; each must print its #| lines.

Commit as me, one commit, no Co-Authored-By or other trailers, message:
  F32.read and F32.show give the C lane's answer on JS
Push the branch to my fork. Do not open the pull request; I will.
```

The PR description is `PR.md` in this folder.
