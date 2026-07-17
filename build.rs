fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/debugproxy.ico");
        res.set("FileDescription", "Debug Proxy - HTTP proxy with TUI");
        res.set("ProductName", "Debug Proxy");
        res.set("CompanyName", "debugproxy");
        match res.compile() {
            Ok(()) => println!("cargo:warning=Windows icon and metadata embedded successfully"),
            Err(e) => println!("cargo:warning=Failed to embed Windows resources: {e}"),
        }
    } else {
        println!("cargo:warning=Skipping Windows resources (target_os={target_os})");
    }
}
