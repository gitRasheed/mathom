#!/bin/sh
# The F32.show sweep, three lanes at once over every decimal-tie f32 and the
# stratified sample: the original patch on bun, the corrected patch on bun
# and on node, each against the C f32_text (ties_c.txt, samp_c.txt).
cd "$(dirname "$0")"
[ -s samp_c.txt ] || ./oracle show < samp.txt > samp_c.txt
run() { eng=$1; funcs=$2; tag=$3
  for set in ties samp; do
    $eng drive.js $funcs show < $set.txt > ${set}_$tag.txt
    echo "$set $tag vs C: $(paste -d'|' $set.txt ${set}_c.txt ${set}_$tag.txt | awk -F'|' '$2!=$3' | wc -l) of $(wc -l < $set.txt)"
  done
}
run bun funcs_ours.js ours_bun > sweep2_ours_bun.log 2>&1 &
run bun funcs_fix.js fix_bun > sweep2_fix_bun.log 2>&1 &
run node funcs_fix.js fix_node > sweep2_fix_node.log 2>&1 &
wait
cat sweep2_ours_bun.log sweep2_fix_bun.log sweep2_fix_node.log
