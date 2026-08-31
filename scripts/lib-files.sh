#!/bin/sh
# List every file that the checks cover.
# Use git when it is available, so ignored files stay out.
set -eu
# `node_modules` holds code that the project did not write, and a `build`
# directory holds what a build produced. The style rules bind what this project
# writes, so neither one goes through them. A `.gitignore` keeps both out of
# the git listing already.
if [ -d .git ] && command -v git >/dev/null 2>&1; then
    git ls-files | grep -v '/node_modules/'
else
    find . \
        -path ./target -prune -o \
        -path ./.git -prune -o \
        -name build -prune -o \
        -name node_modules -prune -o \
        -type f \
        \( -name '*.rs' -o -name '*.md' -o -name '*.toml' -o -name '*.c' \
           -o -name '*.h' -o -name '*.sh' -o -name '*.lark' -o -name '*.ebnf' \
           -o -name '*.yml' -o -name '*.yaml' -o -name '*.json' \) \
        -print | sed 's|^\./||'
fi
