<div align="center">
 <img src="./assets/images/logo.png" alt="RustCV Logo" width="200" height="auto" />

# 📷 RustCV

[简体中文](README_zh.md) | English

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/your-repo/rustcv)
[![Platform](https://img.shields.io/badge/platform-Linux-blue)](https://github.com/your-repo/rustcv)
[![License](https://img.shields.io/badge/license-MIT-yellow)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-edition%202021-orange)](https://www.rust-lang.org/)

**RustCV** is a high-performance, native Rust computer vision library designed to be a modern alternative to OpenCV. By leveraging Rust's memory safety and zero-cost abstractions, we provide a seamless, "C++-dependency-free" experience for vision developers and robotics engineers.

</div>

# RustCV 🦀

**RustCV** is a high-performance, native Rust computer vision library designed to be a modern alternative to OpenCV. By leveraging Rust's memory safety and zero-cost abstractions, we provide a seamless, "C++-dependency-free" experience for vision developers and robotics engineers.

## 📖 Introduction

- **OpenCV Parity**: Provides familiar APIs such as `VideoCapture`, `Mat`, and `imshow`, significantly reducing migration costs for developers coming from C++.
- **Hidden Complexity**: The backend is powered by a **Tokio-based asynchronous driver**, while exposing a clean, synchronous blocking interface. You get the performance of async I/O without the boilerplate of `async/await`.
- **Zero-Copy Design**: Implements intelligent **Buffer Swapping** technology to achieve zero-copy data flow from kernel-space drivers directly to user-space `Mat` structures.

## ✨ Key Features

- **🦀 Rust Native**: Written entirely in Rust. Say goodbye to C++ shared library "dependency hell" and complex CMake configurations.
- **⚡ High Performance**:
- Integrated **Lazy Global Runtime** for automatic management of asynchronous drivers.
- Supports **Stride memory layout**, allowing direct mapping of hardware buffers.

- **🎨 Out-of-the-box Functionality**:
- **VideoIO**: Native support for **V4L2 (Linux)**; AVFoundation (macOS) is currently WIP.
- **HighGUI**: Lightweight, cross-platform windowing based on `minifb` for real-time debugging.
- **ImgProc**: Built-in drawing primitives (rectangles, text) and real-time FPS calculation.
- **ImgCodecs**: Integrated with `image-rs` for reading/writing all major image formats.

- **🛠️ Strong-Typed Configuration**: No more "magic numbers." Use ergonomic APIs like `cap.set_resolution(1280, 720)`.

## 📦 Quick Start

Add RustCV to your `Cargo.toml`:

```toml
[dependencies]
rustcv = "0.1"
```

### Basic Example: Camera Stream

```rust
use anyhow::Result;
use rustcv::prelude::*; // Import VideoCapture, Mat
use rustcv::highgui;    // Import GUI
use rustcv::imgproc;    // Import Drawing/Image Processing

fn main() -> Result<()> { (V4L2 on Linux)
    // 1. Open the camera (index 0)
    // The runtime is managed internally; no #[tokio::main] macro is required.
    let mut cap = VideoCapture::new(0)?;

    // 2. (Optional) Set resolution
    cap.set_resolution(640, 480)?;

    let mut frame = Mat::empty();

    println!("🎥 Start capturing... Press ESC to exit.");

    // 3. Main capture loop
    while cap.read(&mut frame)? {
        if frame.is_empty() { continue; }

        // --- Image Processing ---
        // Draw resolution info in the top-left corner
        imgproc::put_text(
            &mut frame,
            &format!("Res: {}x{}", frame.cols, frame.rows),
            imgproc::Point::new(10, 30),
            1.0,
            imgproc::Scalar::new(0, 0, 255) // Red
        );

        // Draw a green rectangle
        imgproc::rectangle(
            &mut frame,
            imgproc::Rect::new(200, 200, 300, 300),
            imgproc::Scalar::new(0, 255, 0), // Green
            2
        );

        // Display window
        highgui::imshow("RustCV Demo", &frame)?;

        // Input Handling
        if highgui::wait_key(1)? == 27 { // ESC
            break;
        }
    }

    Ok(())
}
```

Run

```bash
cargo run -p rustcv --example demo
```

![RustCV Demo](/assets/images/demo.png)

## 🤝 Contributing

We welcome all forms of contribution! Whether it's porting a new algorithm, improving documentation, or testing on new hardware.

1. Fork the repository.
2. Create your feature branch (`git checkout -b feature/AmazingFeature`).
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`).
4. Push to the branch (`git push origin feature/AmazingFeature`).
5. Open a Pull Request.

---

Built with ❤️ by the **RustCV Community**.
