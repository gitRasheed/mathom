#!/bin/sh
# After show_sweep.sh: the corrected patch's f32_show over the same sets.
cd "$(dirname "$0")"
while ! grep -q "samp ours_node" show_sweep.log 2>/dev/null; do sleep 20; done
for set in ties samp; do
  bun drive.js funcs_fix.js show < $set.txt > ${set}_fix_bun.txt
  node drive.js funcs_fix.js show < $set.txt > ${set}_fix_node.txt
  for v in fix_bun fix_node; do
    echo "$set $v vs C: $(paste -d'|' $set.txt ${set}_c.txt ${set}_$v.txt | awk -F'|' '$2!=$3' | wc -l) of $(wc -l < $set.txt)"
  done
done
# digit-count histogram of the clean lane's tie mismatches
paste -d'|' ties.txt ties_c.txt ties_clean_bun.txt | awk -F'|' '$2!=$3' > ties_clean_diff.txt
awk -F'|' '{n=$2; gsub(/e.*/,"",n); gsub(/[^0-9]/,"",n); sub(/^0+/,"",n); print length(n)}' ties_clean_diff.txt | sort | uniq -c > ties_clean_digits.txt
grep -c e ties_clean_diff.txt > ties_clean_expform.txt
