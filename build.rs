fn main() {
    let target = std::env::var("TARGET").unwrap();
    println!("cargo:rustc-env=RVX_TARGET={target}");
}
