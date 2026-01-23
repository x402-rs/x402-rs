// Copyright (c) Aptos Foundation
// Patched version that removes disable_lifo_slot() call for tokio 1.45+ compatibility

use rayon::{ThreadPool, ThreadPoolBuilder};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::runtime::{Builder, Runtime};

const MAX_THREAD_NAME_LENGTH: usize = 12;

pub fn spawn_named_runtime(thread_name: String, num_worker_threads: Option<usize>) -> Runtime {
    spawn_named_runtime_with_start_hook(thread_name, num_worker_threads, || {})
}

pub fn spawn_named_runtime_with_start_hook<F>(
    thread_name: String,
    num_worker_threads: Option<usize>,
    on_thread_start: F,
) -> Runtime
where
    F: Fn() + Send + Sync + 'static,
{
    const MAX_BLOCKING_THREADS: usize = 64;

    if thread_name.len() > MAX_THREAD_NAME_LENGTH {
        panic!(
            "The given runtime thread name is too long! Max length: {}, given name: {}",
            MAX_THREAD_NAME_LENGTH, thread_name
        );
    }

    let atomic_id = AtomicUsize::new(0);
    let thread_name_clone = thread_name.clone();
    let mut builder = Builder::new_multi_thread();
    builder
        .thread_name_fn(move || {
            let id = atomic_id.fetch_add(1, Ordering::SeqCst);
            format!("{}-{}", thread_name_clone, id)
        })
        .on_thread_start(on_thread_start)
        // NOTE: disable_lifo_slot() removed for tokio 1.45+ compatibility
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .enable_all();
    if let Some(num_worker_threads) = num_worker_threads {
        builder.worker_threads(num_worker_threads);
    }

    builder.build().unwrap_or_else(|error| {
        panic!(
            "Failed to spawn named runtime! Name: {:?}, Error: {:?}",
            thread_name, error
        )
    })
}

pub fn spawn_rayon_thread_pool(
    thread_name: String,
    num_threads: Option<usize>,
) -> ThreadPool {
    spawn_rayon_thread_pool_with_start_hook(thread_name, num_threads, || {})
}

pub fn spawn_rayon_thread_pool_with_start_hook<F>(
    thread_name: String,
    num_threads: Option<usize>,
    start_handler: F,
) -> ThreadPool
where
    F: Fn() + Send + Sync + 'static,
{
    if thread_name.len() > MAX_THREAD_NAME_LENGTH {
        panic!(
            "The given rayon pool thread name is too long! Max length: {}, given name: {}",
            MAX_THREAD_NAME_LENGTH, thread_name
        );
    }

    let atomic_id = AtomicUsize::new(0);
    let mut builder = ThreadPoolBuilder::new()
        .thread_name(move |_| {
            let id = atomic_id.fetch_add(1, Ordering::SeqCst);
            format!("{}-{}", thread_name, id)
        })
        .start_handler(move |_| start_handler());
    if let Some(num_threads) = num_threads {
        builder = builder.num_threads(num_threads);
    }

    builder.build().unwrap_or_else(|error| {
        panic!(
            "Failed to spawn rayon pool! Error: {:?}",
            error
        )
    })
}
