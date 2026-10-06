//! Embeds Windows resources.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=packaging/windows/spotifast.ico");
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("packaging/windows/spotifast.ico")
            .set("ProductName", "Spotifast")
            .set("FileDescription", "A native Spotify client");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=Windows resources not embedded: {error}");
        }
    }
}
