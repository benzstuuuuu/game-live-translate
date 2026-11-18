fn main() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    // CLT 26.6 keeps the Swift back-deploy libs here, not under Toolchains/ where the crates look.
    println!("cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/swift/macosx");
}
