fn main() {
    // apple-vision links Swift back-deploy libs; CLT keeps them here, not where the crate looks.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!(
        "cargo:rustc-link-search=native=/Library/Developer/CommandLineTools/usr/lib/swift/macosx"
    );
}
