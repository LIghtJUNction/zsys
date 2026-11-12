#[cfg(all(feature = "std", target_os = "linux"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};
    use zsys::{linux::MemoryCollector, sampling::Cadence};

    let mut memory = MemoryCollector::new();
    let mut cadence = Cadence::new(10_000_000); // 10 ms, caller-controlled
    let start = Instant::now();
    for _ in 0..100 {
        let now = start.elapsed().as_nanos() as u64;
        if cadence.is_due(now) {
            let sample = memory.refresh()?;
            cadence.mark_success(now);
            println!("{} {}", sample.sample.sequence, sample.counters.available_bytes);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

#[cfg(not(all(feature = "std", target_os = "linux")))]
fn main() {}
