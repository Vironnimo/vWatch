fn main() {
    #[cfg(feature = "desktop")]
    {
        // Tauri tracks its config, but an icon-only change must also rebuild
        // the Windows resources linked into both executables.
        println!("cargo:rerun-if-changed=icons/icon.ico");
        tauri_build::build()
    }
}
