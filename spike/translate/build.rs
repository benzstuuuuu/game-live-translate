// The doom-fish crates link Swift; the binary must find the OS Swift runtime at load time.
fn main() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}
