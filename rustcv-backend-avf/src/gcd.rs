use dispatch2::DispatchQueue;

pub fn get_global_queue() -> &'static DispatchQueue {
    // Return main queue for now to fix build issues.
    DispatchQueue::main()
}
