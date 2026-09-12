fn main() {
    // `tauri_build` embeds `icons/icon.ico` into the Windows executable, but it
    // only declares the configuration files as build inputs. Without this,
    // replacing the app icon leaves the previous resource linked into the
    // binary, so the window, taskbar, and file icon never change.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
