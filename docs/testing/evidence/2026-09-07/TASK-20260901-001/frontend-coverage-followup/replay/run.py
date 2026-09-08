from pathlib import Path
import subprocess, tarfile
p = Path(__file__).resolve().parent.parent
root = next(parent for parent in p.parents if (parent / "src-tauri/Cargo.toml").is_file())
with tarfile.open(p / "resources/frontend-source.tar.gz") as a:
    for f in a:
        if f.isfile():
            assert a.extractfile(f).read() == (root / f.name).read_bytes(), f.name
for command in [
    ["deno", "task", "check:coverage:frontend", "--maxWorkers=2"],
    ["deno", "task", "typecheck"],
    ["deno", "run", "-A", "node_modules/eslint/bin/eslint.js", "src/features/listeners/socket-listener-model.test.ts", "src/features/listeners/socket-processing-card.test.tsx", "--max-warnings=0"],
]:
    subprocess.run(command, cwd=root, check=True)
