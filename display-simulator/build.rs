fn main() {
    println!("cargo:rustc-env=GIT_SHORT=simulator");
    println!("cargo:rustc-env=GIT_DIRTY=false");
}
