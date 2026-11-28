#!/bin/bash
# The whole Phase 6 matrix, each group of runs started only at a 1-minute load of 3 or less:
#   tools/native-qa/phase6_matrix.sh <out-dir>   (env: see phase6.py; target/q6/env.sh sets it)
# full matrix: $1 = out dir
out=$1
cd /home/infernalsugar/Drive/Projects/pp-p6-qa && . target/q6/env.sh
quiet() { local i; for i in $(seq 120); do l=$(cut -d' ' -f1 /proc/loadavg); awk -v l=$l 'BEGIN{exit !(l<=3)}' && return; sleep 15; done; }
run() { quiet; echo "$(date +%T) $(cut -d' ' -f1-3 /proc/loadavg) $*" >> $out/runs.log; timeout 1500 python3 -I tools/native-qa/phase6.py $out "$@" >/dev/null 2>&1; }
mkdir -p $out
run x-first-strokes "x-first-strokes#2" "x-first-strokes#3"
run x-timeline-merge
run x-timeline-roundtrip-native-hosts x-timeline-roundtrip-studio-hosts x-join-studio-file
run x-groups studio-groups x-big-erase
run studio-wire studio-pair studio-crowd studio-garbage studio-revoke "studio-revoke#2"
run x-offline5 "x-offline5#2" "x-offline5#3"
run x-offline40
run "x-host-leaves-studio#2" "x-host-leaves-studio#3"
run x-access x-undo x-garbage-native x-host-leaves-studio x-host-leaves-native x-solo x-join-midstroke
run x-join4k-native-native x-join4k-native-joins x-join4k-studio-joins studio-join4k
for hz in 60 165; do for i in 1 2 3; do
  PP_HZ=$hz run "x-pair-native-hosts#$hz-$i" "x-pair-studio-hosts#$hz-$i"
  PP_HZ=$hz run "x-crowd#$hz-$i"
  PP_HZ=$hz run "x-crowd-peers#$hz-$i"
  PP_HZ=$hz run "studio-pair#$hz-$i"
done; PP_HZ=$hz run "x-import4k#$hz"; done
echo DONE >> $out/runs.log
