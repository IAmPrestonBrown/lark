#!/bin/sh
# Lists every `.lark` file that the formatter owns.
#
# Three groups stay out. A `parse` fixture holds a deliberate syntax error, and
# the formatter has nothing meaningful to say about text that does not parse. A
# `ui` fixture anchors each expected diagnostic to a line. An `lsp` fixture
# marks a cursor position with `<|>`. Formatting any of the three moves what
# the fixture points at.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

sh scripts/lib-files.sh | grep '\.lark$' | grep -vE '^tests/(corpus/parse|ui|lsp)/'
