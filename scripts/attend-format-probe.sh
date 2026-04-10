#!/usr/bin/env bash
# Test what structured formats survive Monitor's event delivery
set -euo pipefail

echo "test 1: plain text"
sleep 1

echo "<attend action=\"observe\">xml with angle brackets</attend>"
sleep 1

echo "[attend action=observe]bracket style[/attend]"
sleep 1

echo "---attend action=observe--- yaml-ish delimiters"
sleep 1

echo "{\"sensor\":\"test\",\"action\":\"observe\",\"msg\":\"json format\"}"
sleep 1

echo "attend::observe::context_pressure — double-colon delimited"
sleep 1

echo "<![CDATA[attend sensor=test action=observe]]>"
sleep 1

echo "<!-- attend action=observe -->comment style"
sleep 1

echo "done"
