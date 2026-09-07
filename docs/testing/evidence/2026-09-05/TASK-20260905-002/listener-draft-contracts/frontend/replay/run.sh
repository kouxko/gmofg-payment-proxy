#!/bin/sh
set -eu
deno task test src/features/listeners
deno task typecheck
deno task lint
deno run -A scripts/check-frontend-boundaries.mjs
deno run -A scripts/check-source-file-sizes.mjs
