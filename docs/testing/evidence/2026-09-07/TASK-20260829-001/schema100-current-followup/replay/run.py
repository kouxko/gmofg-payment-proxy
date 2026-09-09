from pathlib import Path
import tarfile, subprocess
p = Path(__file__).resolve().parent.parent
root = next(parent for parent in p.parents if (parent / "src-tauri/Cargo.toml").is_file())
for name in ["current-source.tar.gz", "runtime-fixture-source.tar.gz", "checker-source.tar.gz"]:
    with tarfile.open(p / "resources" / name) as archive:
        for item in archive:
            if item.isfile():
                assert archive.extractfile(item).read() == (root / item.name).read_bytes(), item.name
subprocess.run(["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-infrastructure", "--lib", "sqlite::"], cwd=root, check=True)
subprocess.run(["deno", "task", "test:task-20260829-002:phase17"], cwd=root, check=True)
