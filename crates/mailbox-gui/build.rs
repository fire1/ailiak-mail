//! Embed the Ailiak icon into the Windows .exe resource table.
//! macOS/Linux use packager / FreeDesktop icons instead.

fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/img/mailbox.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=winresource failed: {e}");
        }
    }
}
