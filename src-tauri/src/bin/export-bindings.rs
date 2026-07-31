//! 单独生成 Tauri Command 的 TypeScript 类型绑定，供构建或 CI 校验使用。
//!
//! 该工具只写生成文件，不启动桌面窗口；失败立即退出，防止前端继续使用过期类型。

fn main() {
    let path = gmofg_payment_proxy::export_bindings().expect("failed to export bindings");
    println!("{}", path.display());
}
