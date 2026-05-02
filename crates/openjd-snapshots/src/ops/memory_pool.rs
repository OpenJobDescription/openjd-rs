// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MIN_MEMORY_BYTES: usize = 256 * 1024 * 1024; // 256 MB
const MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024 * 1024; // 16 GB

/// Async memory pool that bounds total in-flight data.
///
/// Uses a tokio Semaphore where 1 permit = `PERMIT_GRANULARITY` bytes (4KB).
/// This coarser granularity avoids u32 overflow in `acquire_many_owned`,
/// supporting pools up to ~16TB with u32 permits.
pub(crate) struct MemoryPool {
    semaphore: Arc<Semaphore>,
    max_bytes: usize,
}

/// Bytes per permit. Allocations are rounded up to this granularity.
const PERMIT_GRANULARITY: usize = 4096;

fn bytes_to_permits(bytes: usize) -> u32 {
    bytes.div_ceil(PERMIT_GRANULARITY) as u32
}

impl MemoryPool {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(bytes_to_permits(max_bytes) as usize)),
            max_bytes,
        }
    }

    /// Acquire `size` bytes from the pool. Waits until enough memory is available.
    ///
    /// If `size` exceeds `max_bytes`, it is clamped so that a single large
    /// allocation can always proceed once all other permits are released.
    pub async fn acquire(&self, size: usize) -> OwnedSemaphorePermit {
        let clamped = size.min(self.max_bytes);
        let permits = bytes_to_permits(clamped);
        self.semaphore
            .clone()
            .acquire_many_owned(permits)
            .await
            .expect("semaphore closed")
    }

    #[allow(dead_code)]
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    #[allow(dead_code)]
    pub fn available(&self) -> usize {
        self.semaphore.available_permits() * PERMIT_GRANULARITY
    }
}

/// Formula: `min(16GB, max(256MB, total_ram/4, available_ram - 1GB))`
/// Falls back to 256MB if system memory cannot be detected.
pub(crate) fn default_max_memory_bytes() -> usize {
    match detect_system_memory() {
        Some((total, available)) => {
            let quarter_total = total / 4;
            let available_minus_1gb = available.saturating_sub(1024 * 1024 * 1024);
            MAX_MEMORY_BYTES.min(MIN_MEMORY_BYTES.max(quarter_total).max(available_minus_1gb))
        }
        None => MIN_MEMORY_BYTES,
    }
}

fn detect_system_memory() -> Option<(usize, usize)> {
    #[cfg(target_os = "linux")]
    {
        let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
        let mut total: Option<usize> = None;
        let mut available: Option<usize> = None;
        for line in contents.lines() {
            if line.starts_with("MemTotal:") {
                total = parse_meminfo_kb(line);
            } else if line.starts_with("MemAvailable:") {
                available = parse_meminfo_kb(line);
            }
            if total.is_some() && available.is_some() {
                break;
            }
        }
        return Some((total?, available?));
    }

    #[cfg(target_os = "macos")]
    {
        let total = sysctl_by_name_u64("hw.memsize")? as usize;
        // vm.page_free_count * vm.pagesize approximates available memory
        let page_size = sysctl_by_name_u64("vm.pagesize")? as usize;
        let free_pages = sysctl_by_name_u64("vm.page_free_count")? as usize;
        let available = free_pages * page_size;
        return Some((total, available));
    }

    #[allow(unreachable_code)]
    None
}

#[cfg(target_os = "macos")]
fn sysctl_by_name_u64(name: &str) -> Option<u64> {
    use std::ffi::CString;
    let c_name = CString::new(name).ok()?;
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let ret = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            &mut value as *mut u64 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret == 0 {
        Some(value)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn parse_meminfo_kb(line: &str) -> Option<usize> {
    // Format: "MemTotal:       16384000 kB"
    line.split_whitespace()
        .nth(1)?
        .parse::<usize>()
        .ok()
        .map(|kb| kb * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: usize = PERMIT_GRANULARITY; // 4096

    #[tokio::test]
    async fn acquire_and_release() {
        let pool = MemoryPool::new(10 * G);
        assert_eq!(pool.available(), 10 * G);

        let permit = pool.acquire(4 * G).await;
        assert_eq!(pool.available(), 6 * G);

        drop(permit);
        assert_eq!(pool.available(), 10 * G);
    }

    #[tokio::test]
    async fn acquire_clamps_to_max() {
        let pool = MemoryPool::new(2 * G);
        let permit = pool.acquire(4 * G).await;
        assert_eq!(pool.available(), 0);
        drop(permit);
        assert_eq!(pool.available(), 2 * G);
    }

    #[tokio::test]
    async fn multiple_acquires_block_when_full() {
        let pool = Arc::new(MemoryPool::new(4 * G));

        let p1 = pool.acquire(3 * G).await;
        assert_eq!(pool.available(), G);

        let pool2 = pool.clone();
        let handle = tokio::spawn(async move { pool2.acquire(3 * G).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !handle.is_finished(),
            "should be blocked waiting for memory"
        );

        drop(p1);
        let _p2 = handle.await.unwrap();
        assert_eq!(pool.available(), G);
    }

    #[test]
    fn default_memory_at_least_min() {
        let mem = default_max_memory_bytes();
        assert!(mem >= MIN_MEMORY_BYTES, "got {mem}");
        assert!(mem <= MAX_MEMORY_BYTES, "got {mem}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detect_memory_on_linux() {
        let (total, available) = detect_system_memory().expect("should detect memory on Linux");
        assert!(total > 0);
        assert!(available > 0);
        assert!(available <= total);
    }

    #[tokio::test]
    async fn acquire_exact_limit() {
        let pool = MemoryPool::new(2 * G);
        let p1 = pool.acquire(2 * G).await;
        assert_eq!(pool.available(), 0);

        let pool2 = Arc::new(pool);
        let pool3 = pool2.clone();
        let handle = tokio::spawn(async move { pool3.acquire(G).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!handle.is_finished(), "should be blocked at exact limit");

        drop(p1);
        let _p2 = handle.await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_acquire_release() {
        let pool = Arc::new(MemoryPool::new(4 * G));
        let mut handles = Vec::new();
        for _ in 0..10 {
            let p = pool.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..5 {
                    let permit = p.acquire(G).await;
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                    drop(permit);
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(pool.available(), 4 * G);
    }

    #[test]
    fn test_max_bytes_and_available() {
        let pool = MemoryPool::new(4 * G);
        assert_eq!(pool.max_bytes(), 4 * G);
        assert_eq!(pool.available(), 4 * G);
    }

    #[test]
    fn sub_granularity_rounds_up() {
        let _pool = MemoryPool::new(G);
        // A 1-byte request should still consume 1 permit (= G bytes)
        assert_eq!(bytes_to_permits(1), 1);
        assert_eq!(bytes_to_permits(G), 1);
        assert_eq!(bytes_to_permits(G + 1), 2);
    }

    #[test]
    fn large_values_no_truncation() {
        // 8GB should not truncate — this was the original bug
        let eight_gb = 8usize * 1024 * 1024 * 1024;
        let permits = bytes_to_permits(eight_gb);
        assert_eq!(permits as usize * PERMIT_GRANULARITY, eight_gb);
    }
}
