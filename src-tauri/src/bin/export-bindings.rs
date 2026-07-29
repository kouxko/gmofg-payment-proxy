fn main() {
    let path = gmofg_payment_proxy::export_bindings().expect("failed to export bindings");
    println!("{}", path.display());
}
