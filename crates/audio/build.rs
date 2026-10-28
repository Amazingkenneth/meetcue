fn main() {
    println!("cargo:rerun-if-changed=src/macos/capture.m");
    println!("cargo:rerun-if-changed=src/macos/capture.h");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/macos/capture.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .flag("-Wno-deprecated-declarations")
            .flag("-mmacosx-version-min=13.0")
            .compile("meetcue_capture");
        for framework in [
            "Foundation",
            "AVFoundation",
            "CoreAudio",
            "CoreMedia",
            "ScreenCaptureKit",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
