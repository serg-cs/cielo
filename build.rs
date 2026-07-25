fn main() {
    // Rebuild when browser templates or static assets change.
    println!("cargo:rerun-if-changed=web");
}
