//! Windows-only: embeds `assets/icon.ico` as the `.exe`'s own icon resource
//! (Explorer, taskbar, alt-tab - all read this from the binary itself, not
//! from anything set at runtime). The window/taskbar icon set at runtime via
//! `winit::window::WindowBuilder::with_window_icon` (see `main.rs`) is a
//! separate mechanism that only takes effect once the window exists; this
//! is what shows *before* the app is even running.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        // The .exe's own product name (Explorer's "Details" tab, some
        // shells' alt-tab tooltip) - separate from the in-app window title
        // set at runtime (see `main.rs`'s `WindowBuilder::with_title`).
        res.set("ProductName", "Contra: Rewired");
        res.set("FileDescription", "Contra: Rewired");
        if let Err(e) = res.compile() {
            // Non-fatal: a build without the icon embedded still runs -
            // just without a custom .exe icon. Print instead of panicking
            // so a broken resource compiler (rare, Windows-toolchain-
            // specific) doesn't block `cargo build` entirely.
            println!("cargo:warning=failed to embed .exe icon/metadata: {e}");
        }
    }
}
