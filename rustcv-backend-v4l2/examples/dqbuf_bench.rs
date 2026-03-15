#[cfg(target_os = "linux")]
fn main() {
    use std::time::Instant;
    use v4l::buffer::Type;
    use v4l::io::traits::CaptureStream;
    use v4l::video::Capture;

    let dev = v4l::Device::with_path("/dev/video0").unwrap();

    // Set MJPG 640x480
    let mut fmt = dev.format().unwrap();
    fmt.width = 640;
    fmt.height = 480;
    fmt.fourcc = v4l::FourCC::new(b"MJPG");
    let fmt = dev.set_format(&fmt).unwrap();
    println!("Format: {}x{} {}", fmt.width, fmt.height, fmt.fourcc);

    // Set 30fps
    let params = v4l::video::capture::Parameters::with_fps(30);
    let actual = dev.set_params(&params).unwrap();
    println!(
        "FPS: {}/{}",
        actual.interval.denominator, actual.interval.numerator
    );

    // Disable dynamic framerate
    let _ = dev.set_control(v4l::control::Control {
        id: 0x009a0903,
        value: v4l::control::Value::Boolean(false),
    });

    let mut stream = v4l::io::mmap::Stream::with_buffers(&dev, Type::VideoCapture, 5).unwrap();

    // Warmup
    for _ in 0..10 {
        let _ = CaptureStream::next(&mut stream).unwrap();
    }

    // Benchmark: DQBUF only (no copy)
    let n = 100;
    let start = Instant::now();
    let mut last_seq = 0u32;
    for i in 0..n {
        let (_buf, meta) = CaptureStream::next(&mut stream).unwrap();
        if i == 0 {
            last_seq = meta.sequence;
        }
        if i == n - 1 {
            println!(
                "seq range: {} to {} (delta={})",
                last_seq,
                meta.sequence,
                meta.sequence - last_seq
            );
        }
    }
    let elapsed = start.elapsed();
    println!(
        "No-copy: {} frames in {:.2}s = {:.1} fps",
        n,
        elapsed.as_secs_f64(),
        n as f64 / elapsed.as_secs_f64()
    );

    // Benchmark: DQBUF + copy (like our code)
    let start = Instant::now();
    for _ in 0..n {
        let (buf, _meta) = CaptureStream::next(&mut stream).unwrap();
        let _copy = buf.to_vec(); // 614KB copy
    }
    let elapsed = start.elapsed();
    println!(
        "With full copy: {} frames in {:.2}s = {:.1} fps",
        n,
        elapsed.as_secs_f64(),
        n as f64 / elapsed.as_secs_f64()
    );

    // Benchmark: DQBUF + copy only bytesused
    let start = Instant::now();
    for _ in 0..n {
        let (buf, meta) = CaptureStream::next(&mut stream).unwrap();
        let _copy = buf[..meta.bytesused as usize].to_vec(); // ~88KB copy
    }
    let elapsed = start.elapsed();
    println!(
        "With bytesused copy: {} frames in {:.2}s = {:.1} fps",
        n,
        elapsed.as_secs_f64(),
        n as f64 / elapsed.as_secs_f64()
    );
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("Linux only");
}
