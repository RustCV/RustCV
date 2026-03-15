//! Build script for rustcv-camera.
//! Compiles platform-specific C/ObjC bridge code.

fn main() {
    #[cfg(target_os = "macos")]
    build_avf_bridge();
}

#[cfg(target_os = "macos")]
fn build_avf_bridge() {
    println!("cargo:rerun-if-changed=src/backend/macos/bridge.m");
    println!("cargo:rerun-if-changed=src/backend/macos/bridge.h");

    cc::Build::new()
        .file("src/backend/macos/bridge.m")
        // Enable Automatic Reference Counting — simplifies ObjC memory management.
        .flag("-fobjc-arc")
        // Enable Clang modules for AVFoundation/CoreMedia/CoreVideo headers.
        .flag("-fmodules")
        .compile("avf_bridge");

    // Link system frameworks required by AVFoundation camera capture.
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreVideo");
    println!("cargo:rustc-link-lib=framework=Foundation");
    // Accelerate.framework provides vImageScale_ARGB8888 for SIMD-accelerated
    // resolution scaling when the hardware delivers at a different resolution
    // than the user requested.
    println!("cargo:rustc-link-lib=framework=Accelerate");
}
