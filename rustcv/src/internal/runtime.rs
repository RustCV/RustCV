use std::sync::OnceLock;
use tokio::runtime::Runtime;

// 全局单例 Runtime
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// 获取全局 Runtime，如果不存在则创建
/// 这允许用户不写 #[tokio::main] 也能跑异步驱动
pub fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2) // 这里的 IO 任务不繁重，2个线程足矣
            .thread_name("rustcv-bg-worker")
            .build()
            .expect("Failed to create RustCV background runtime")
    })
}

/// 辅助函数：在后台运行 Future 并阻塞等待结果
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    get_runtime().block_on(future)
}
