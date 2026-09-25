fn main() {
    let triple = std::env::var("TARGET").unwrap_or_else(|_| {
        std::env::var("HOST").expect("HOST or TARGET must be set by cargo")
    });
    println!("cargo:rustc-env=VIDEOFETCH_HOST_TRIPLE={triple}");
    tauri_build::build()
}
