#[cfg(all(feature = "std", target_os = "linux"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut system = zsys::System::new();
    system.refresh_cpu()?;
    system.refresh_memory()?;
    system.refresh_processes()?;

    // Two observations define a CPU interval. Sleeping is a caller policy.
    std::thread::sleep(std::time::Duration::from_millis(250));
    system.refresh_cpu()?;
    system.refresh_processes()?;
    println!("CPU busy: {:?}%", system.cpu().and_then(|cpu| cpu.aggregate_busy_percent()));
    if let Some(memory) = system.memory() {
        println!("Memory used/total: {}/{} bytes", memory.counters.used_bytes(), memory.counters.total_bytes);
    }
    for process in system.processes() {
        println!(
            "{} {} CPU={:?}% RSS={} bytes {:?}",
            process.id.pid,
            String::from_utf8_lossy(&process.name),
            process.cpu_percent,
            process.memory_bytes,
            process.availability,
        );
    }
    Ok(())
}

#[cfg(not(all(feature = "std", target_os = "linux")))]
fn main() {
    eprintln!("The collection example requires Linux and the std feature.");
}
