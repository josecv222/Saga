fn main() {
    println!("cargo:rerun-if-changed=assets/saga.ico");
    #[cfg(windows)]
    if std::path::Path::new("assets/saga.ico").exists() {
        let mut r = winresource::WindowsResource::new();
        r.set_icon("assets/saga.ico");
        r.set("ProductName", "Saga");
        r.set("FileDescription", "Saga");
        let _ = r.compile();
    }
}
