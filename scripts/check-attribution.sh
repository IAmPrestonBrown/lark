#!/bin/sh
# Rule C-1.3: no file names an AI assistant or a language model.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

pattern='claude|anthropic|chatgpt|openai|gpt-[0-9]|copilot|gemini|llama|\
co-authored-by:.*(ai|bot)|generated with|written by an ai|large language model|\
\bllm\b'
pattern=$(printf '%s' "$pattern" | tr -d '\\\n')

fail=0
for f in $(sh scripts/lib-files.sh); do
    case "$f" in
        CLAUDE.md|scripts/check-attribution.sh) continue ;;
    esac
    # A file that states the rule itself is exempt. See .attribution-exempt.
    if [ -f .attribution-exempt ] && grep -qxF "$f" .attribution-exempt; then
        continue
    fi
    if LC_ALL=C grep -qiE "$pattern" "$f"; then
        echo "attribution: $f"
        LC_ALL=C grep -niE "$pattern" "$f" | head -5 | sed 's/^/    /'
        fail=1
    fi
done

if [ -d .git ]; then
    if git log --format='%B%n%(trailers)' -n 200 2>/dev/null | LC_ALL=C grep -qiE "$pattern"; then
        echo "attribution: a commit message in the last 200 commits"
        fail=1
    fi
fi

if [ "$fail" -ne 0 ]; then
    echo "FAIL check-attribution (rule C-1.3)"
    exit 1
fi
echo "ok   check-attribution"
