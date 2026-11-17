fn main() {
    // Re-run build if web assets change so rust-embed picks up updates.
    println!("cargo:rerun-if-changed=web/");
}
