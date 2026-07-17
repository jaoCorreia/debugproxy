fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        match winresource::WindowsResource::new()
            .set_icon("assets/debugproxy.ico")
            .set("FileDescription", "Debug Proxy - HTTP proxy with TUI")
            .set("ProductName", "Debug Proxy")
            .set("CompanyName", "debugproxy")
            .compile()
        {
            Ok(()) => println!("cargo:warning=Windows icon and metadata embedded successfully"),
            Err(e) => println!("cargo:warning=Failed to embed Windows resources: {e}"),
        }
    } else {
        println!("cargo:warning=Skipping Windows resources (target_os={target_os})");
    }
}
