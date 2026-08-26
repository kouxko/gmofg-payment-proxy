# 准备步骤

1. 从仓库根目录确认 `git rev-parse HEAD`。
2. 确认 `inputs/task-related-files.txt` 的 29 个路径存在。
3. 比较七个活动 fixture 与 `resources/active-fixtures/` 副本的 SHA-256。
4. 确认 G033 白名单没有 staged 变更；共享工作树的其他任务改动不纳入本用例。
5. Rust 命令始终从仓库根目录使用 `--manifest-path src-tauri/Cargo.toml`。

不需要启动 App、Listener、SQLite、Android、外部包或远程服务。
