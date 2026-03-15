/// Camera capture benchmarks.
/// 摄像头采集基准测试。
///
/// These benchmarks require a real camera connected to /dev/video0.
/// They measure actual capture performance, not synthetic workloads.
/// 这些基准测试需要连接到 /dev/video0 的真实摄像头。
/// 测量的是实际采集性能，而非合成负载。
///
/// Run with:
///   cargo bench --bench capture -p rustcv-camera --features turbojpeg
///
/// Note: criterion's statistical analysis is less meaningful for camera benchmarks
/// because frame timing is dominated by the camera's hardware frame rate (e.g., 33ms at 30fps).
/// We use criterion mainly for consistent reporting format.
///
/// 注意：criterion 的统计分析对摄像头基准测试意义不大，
/// 因为帧时间主要由摄像头硬件帧率决定（如 30fps 时为 33ms）。
/// 我们使用 criterion 主要是为了一致的报告格式。
use criterion::{criterion_group, criterion_main, Criterion};

use rustcv_camera::{Camera, CameraConfig, Mat, VideoCapture};
use std::time::{Duration, Instant};

/// Benchmark 1: Raw DQBUF throughput (zero-copy, no decode).
/// 基准测试 1：原始 DQBUF 吞吐量（零拷贝，无解码）。
///
/// Measures the maximum frame rate achievable by the capture engine alone.
/// This is the theoretical upper bound — no format conversion overhead.
/// 测量仅取帧引擎能达到的最大帧率。这是理论上限 —— 无格式转换开销。
fn bench_raw_dqbuf(c: &mut Criterion) {
    let mut cam = match Camera::open(0) {
        Ok(cam) => cam,
        Err(e) => {
            eprintln!("Skipping bench_raw_dqbuf: {}", e);
            return;
        }
    };

    let mut group = c.benchmark_group("capture");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("raw_dqbuf_30fps", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let _frame = cam.next_frame().unwrap();
                // Frame is dropped here — buffer returned on next iteration.
                // 帧在此处被 drop —— 缓冲区在下次迭代时归还。
            }
            start.elapsed()
        });
    });

    group.finish();
}

/// Benchmark 2: DQBUF + MJPEG decode (turbojpeg).
/// 基准测试 2：DQBUF + MJPEG 解码（turbojpeg）。
///
/// Measures the full VideoCapture::read() pipeline including JPEG decompression.
/// This is the realistic FPS for users who want decoded BGR frames.
/// 测量包含 JPEG 解压的完整 VideoCapture::read() 管线。
/// 这是用户获取解码 BGR 帧时的实际 FPS。
fn bench_videocapture_read(c: &mut Criterion) {
    let mut cap = match VideoCapture::open(0) {
        Ok(cap) => cap,
        Err(e) => {
            eprintln!("Skipping bench_videocapture_read: {}", e);
            return;
        }
    };

    let mut mat = Mat::new();
    let mut group = c.benchmark_group("capture");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("videocapture_read_30fps", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                cap.read(&mut mat).unwrap();
            }
            start.elapsed()
        });
    });

    group.finish();
}

/// Benchmark 3: Multi-resolution comparison.
/// 基准测试 3：多分辨率对比。
///
/// Compares capture FPS at 480p, 720p, and 1080p.
/// 对比 480p、720p、1080p 下的采集 FPS。
fn bench_resolutions(c: &mut Criterion) {
    let resolutions = [
        (640u32, 480u32, "480p"),
        (1280, 720, "720p"),
        (1920, 1080, "1080p"),
    ];

    let mut group = c.benchmark_group("resolution");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(3));

    for (w, h, label) in resolutions {
        let config = CameraConfig::new().resolution(w, h).fps(30);
        let mut cap = match VideoCapture::open_with(0, config) {
            Ok(cap) => cap,
            Err(e) => {
                eprintln!("Skipping {}: {}", label, e);
                continue;
            }
        };
        let mut mat = Mat::new();

        group.bench_function(label, |b| {
            b.iter_custom(|iters| {
                let start = Instant::now();
                for _ in 0..iters {
                    cap.read(&mut mat).unwrap();
                }
                start.elapsed()
            });
        });
    }

    group.finish();
}

/// Benchmark 4: Detailed frame statistics (custom, not criterion).
/// 基准测试 4：详细帧统计（自定义，非 criterion）。
///
/// Runs after criterion benchmarks to print a human-readable report
/// with FPS, P99 latency, drop rate, and max DQBUF time.
/// 在 criterion 基准测试后运行，打印包含 FPS、P99 延迟、
/// 丢帧率和最大 DQBUF 耗时的可读报告。
fn bench_detailed_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("frame_stats_100frames", |b| {
        b.iter_custom(|_iters| {
            let mut cam = match Camera::open(0) {
                Ok(cam) => cam,
                Err(_) => return Duration::from_secs(0),
            };

            let n = 100u64;
            let mut intervals = Vec::with_capacity(n as usize);
            let mut last_seq: u64 = 0;
            let mut dropped: u64 = 0;
            let mut prev_time = Instant::now();

            let start = Instant::now();
            for i in 0..n {
                let frame = cam.next_frame().unwrap();
                let now = Instant::now();

                if i > 0 {
                    intervals.push(now.duration_since(prev_time));
                    if frame.sequence() > last_seq + 1 {
                        dropped += frame.sequence() - last_seq - 1;
                    }
                }
                last_seq = frame.sequence();
                prev_time = now;
            }
            let total = start.elapsed();

            // Sort intervals for percentile calculation.
            // 排序间隔用于百分位数计算。
            intervals.sort();
            let p99_idx = (intervals.len() as f64 * 0.99) as usize;
            let p99 = intervals
                .get(p99_idx.min(intervals.len() - 1))
                .copied()
                .unwrap_or_default();
            let max_interval = intervals.last().copied().unwrap_or_default();
            let avg_fps = n as f64 / total.as_secs_f64();

            eprintln!();
            eprintln!("┌─────────────── Frame Statistics ───────────────┐");
            eprintln!("│ Frames captured:  {:>6}                       │", n);
            eprintln!("│ Frames dropped:   {:>6}                       │", dropped);
            eprintln!(
                "│ Drop rate:        {:>5.1}%                        │",
                dropped as f64 / (n + dropped) as f64 * 100.0
            );
            eprintln!(
                "│ Average FPS:      {:>6.1}                       │",
                avg_fps
            );
            eprintln!(
                "│ P99 interval:     {:>6.2}ms                     │",
                p99.as_secs_f64() * 1000.0
            );
            eprintln!(
                "│ Max interval:     {:>6.2}ms                     │",
                max_interval.as_secs_f64() * 1000.0
            );
            eprintln!("└────────────────────────────────────────────────┘");

            total
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_raw_dqbuf,
    bench_videocapture_read,
    bench_resolutions,
    bench_detailed_stats,
);
criterion_main!(benches);
