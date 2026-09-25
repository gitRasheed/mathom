#!/bin/sh
# The F32.show sweep: every decimal-tie f32 (tiecands), then a stratified
# sample of 4096 mantissas per exponent and sign, through the C f32_text
# and the JS f32_show of the clean and the patched comp.ts, on bun and node.
cd "$(dirname "$0")"
./tiecands > ties.txt 2> ties.count
./tiecands sample 4096 > samp.txt
for set in ties samp; do
  ./oracle show < $set.txt > ${set}_c.txt
  bun drive.js funcs_clean.js show < $set.txt > ${set}_clean_bun.txt
  bun drive.js funcs_ours.js show < $set.txt > ${set}_ours_bun.txt
  node drive.js funcs_ours.js show < $set.txt > ${set}_ours_node.txt
  for v in clean_bun ours_bun ours_node; do
    echo "$set $v vs C: $(paste -d'|' $set.txt ${set}_c.txt ${set}_$v.txt | awk -F'|' '$2!=$3' | wc -l) of $(wc -l < $set.txt)"
  done
done
