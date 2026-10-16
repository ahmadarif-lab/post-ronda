fn main() {
    println!("cargo:rerun-if-changed=assets/PostRonda.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && (cfg!(windows) || std::env::var_os("WINDRES").is_some())
    {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/PostRonda.ico")
            .set("ProductName", "Post Ronda")
            .set("FileDescription", "Post Ronda native HTTP client");
        resource
            .compile()
            .expect("Windows icon and version resources");
    }
}
