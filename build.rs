fn main() {
    println!("cargo:rerun-if-changed=assets/wanalyzer.manifest");
    println!("cargo:rerun-if-changed=assets/wanalyzer.ico");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set("ProductName", "WANanalyzer")
        .set(
            "FileDescription",
            "Network latency and per-hop packet loss monitor",
        )
        .set_icon("assets/wanalyzer.ico")
        .set_manifest_file("assets/wanalyzer.manifest");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        resource
            .set_windres_path("x86_64-w64-mingw32-windres")
            .set_ar_path("llvm-ar");
    }
    resource
        .compile()
        .expect("Compile Windows version metadata and asInvoker manifest");
}
