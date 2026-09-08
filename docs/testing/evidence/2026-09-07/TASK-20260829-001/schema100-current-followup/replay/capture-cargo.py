#!/usr/bin/env python3
from pathlib import Path
import os, subprocess, sys
args = sys.argv[1:]
r = subprocess.run(["cargo", *args])
if r.returncode == 0 and "--target-dir" in args:
    target = Path(args[args.index("--target-dir") + 1])
    source = target / "wasm32-wasip2/debug/intercept_proxy_iso8583_ascii_standard_component.wasm"
    dest = Path(os.environ["DB100_CAPTURE_DIR"])
    dest.mkdir(parents=True, exist_ok=True)
    with (dest / source.name).open("xb") as output:
        output.write(source.read_bytes())
sys.exit(r.returncode)
