fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico")
        .set("ProductName", "Dual Audio")
        .set("FileDescription", "Dual Audio")
        .set("LegalCopyright", "Apache-2.0");
    if let Err(e) = res.compile() {
        // Cross builds without a resource compiler still work, just without an icon.
        println!("cargo:warning=could not embed the Windows icon: {e}");
    }
}
