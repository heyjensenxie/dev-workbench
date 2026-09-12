// Prevents an extra console window on Windows in release. Debug builds keep the
// console so `pnpm dev` can still print the runtime's output. DO NOT REMOVE.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dev_workbench_lib::run()
}
