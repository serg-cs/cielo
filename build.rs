fn main() {
    // Rebuild when files are added to or removed from the embedded site.
    println!("cargo:rerun-if-changed=site");
}
