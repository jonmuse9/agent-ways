#!/usr/bin/env bash
# Research way macro — show available skills for investigation tasks
#
# Research is mostly greenfield (no official Anthropic skills),
# but some knowledge-work plugins are relevant.

installed=""
if command -v claude >/dev/null 2>&1; then
  installed=$(claude plugin list 2>/dev/null | grep '✔ enabled' || true)
fi

echo ""
echo "## Skills for Research"
echo ""
echo "| Task | Skill | Installed | Install command |"
echo "|------|-------|-----------|-----------------|"

for entry in \
  "Research synthesis|research-synthesis|knowledge-work-plugins" \
  "User research|user-research|knowledge-work-plugins" \
  "Data exploration|explore-data|knowledge-work-plugins" \
  "Statistical analysis|statistical-analysis|knowledge-work-plugins"; do

  IFS='|' read -r task skill marketplace <<< "$entry"
  if echo "$installed" | grep -q "$skill"; then
    status="yes"
  else
    status="no"
  fi
  echo "| ${task} | \`${skill}\` | ${status} | \`claude plugin install ${skill}@${marketplace}\` |"
done

echo ""
echo "Most research uses built-in tools (WebSearch, WebFetch, Read, Grep). Skills add structured workflows on top."
