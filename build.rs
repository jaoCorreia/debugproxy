fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        if let Err(e) = winresource::WindowsResource::new()
            .set_icon("assets/debugproxy.ico")
            .set("FileDescription", "Debug Proxy - HTTP proxy with TUI")
            .set("ProductName", "Debug Proxy")
            .set("CompanyName", "debugproxy")
            .compile()
        {
            eprintln!("warning: failed to embed Windows resources: {e}");
        }
    }
}
