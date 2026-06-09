#!/usr/bin/env bash
# Refresh CLAUDE.md context after compaction
# This script ensures Claude re-reads both user and project scope configurations

echo "🔄 Refreshing CLAUDE.md context post-compaction..."

# Force re-read of user scope CLAUDE.md
if [ -f "$HOME/.claude/CLAUDE.md" ]; then
    touch "$HOME/.claude/CLAUDE.md"
    echo "✓ User scope CLAUDE.md refreshed"
fi

# Hint: Project-scoped CLAUDE.md files should be discovered and read as needed
# The updated user CLAUDE.md will instruct Claude to locate and read them
echo "📁 Project scope CLAUDE.md discovery will be handled by updated instructions"

echo "✅ Context refresh complete - Claude will re-read configurations on next interaction"
exit 0