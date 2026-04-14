use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MIN_MEMORY_BYTES: usize = 256 * 1024 * 1024; // 256 MB
const MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024 * 1024; // 16 GB

/// Async memory pool that bounds total in-flight data.
///
/// Uses a tokio Semaphore where 1 permit = 1 byte. Callers acquire
/// permits before reading data into memory and release them (by dropping
/// the permit) after the data has been uploaded/written.
pub(crate) struct MemoryPool {
    semaphore: Arc<Semaphore>,
    max_bytes: usize,
}

impl MemoryPool {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_bytes)),
            max_bytes,
        }
    }

    /// Acquire `size` bytes from the pool. Waits until enough memory is available.
    ///
    /// If `size` exceeds `max_bytes`, it is clamped so that a single large
    /// allocation can always proceed once all other permits are released.
    pub async fn acquire(&self, size: usize) -> OwnedSemaphorePermit {
        let clamped = size.min(self.max_bytes) as u32;
        self.semaphore
            .clone()
            .acquire_many_owned(clamped)
            .await
            .expect("semaphore closed")
    }

    #[allow(dead_code)]
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    #[allow(dead_code)]
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }
}

/// Calculate the default memory limit for the pipeline.
///
/// Returns the number of available CPU cores, defaulting to 4 if detection fails.
pub(crate) fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
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

    #[allow(unreachable_code)]
    None
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

    #[tokio::test]
    async fn acquire_and_release() {
        let pool = MemoryPool::new(1000);
        assert_eq!(pool.available(), 1000);

        let permit = pool.acquire(400).await;
        assert_eq!(pool.available(), 600);

        drop(permit);
        assert_eq!(pool.available(), 1000);
    }

    #[tokio::test]
    async fn acquire_clamps_to_max() {
        let pool = MemoryPool::new(100);
        let permit = pool.acquire(200).await;
        assert_eq!(pool.available(), 0);
        drop(permit);
        assert_eq!(pool.available(), 100);
    }

    #[tokio::test]
    async fn multiple_acquires_block_when_full() {
        let pool = Arc::new(MemoryPool::new(100));

        let p1 = pool.acquire(80).await;
        assert_eq!(pool.available(), 20);

        let pool2 = pool.clone();
        let handle = tokio::spawn(async move { pool2.acquire(80).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !handle.is_finished(),
            "should be blocked waiting for memory"
        );

        drop(p1);
        let _p2 = handle.await.unwrap();
        assert_eq!(pool.available(), 20);
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
        let pool = MemoryPool::new(100);
        let p1 = pool.acquire(100).await;
        assert_eq!(pool.available(), 0);

        let pool2 = Arc::new(pool);
        let pool3 = pool2.clone();
        let handle = tokio::spawn(async move { pool3.acquire(1).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!handle.is_finished(), "should be blocked at exact limit");

        drop(p1);
        let _p2 = handle.await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_acquire_release() {
        let pool = Arc::new(MemoryPool::new(200));
        let mut handles = Vec::new();
        for _ in 0..10 {
            let p = pool.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..5 {
                    let permit = p.acquire(50).await;
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                    drop(permit);
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(pool.available(), 200);
    }

    #[test]
    fn test_max_bytes_and_available() {
        let pool = MemoryPool::new(1024);
        assert_eq!(pool.max_bytes(), 1024);
        assert_eq!(pool.available(), 1024);
    }
}
