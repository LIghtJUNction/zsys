#![cfg(all(feature = "std", target_os = "linux"))]

use zsys::System;

#[test]
fn refreshes_are_independent_and_snapshots_are_borrowed() {
    let mut system = System::new();
    assert!(system.cpu().is_none());
    assert!(system.memory().is_none());
    assert!(system.process_table().is_none());
    assert!(system.networks().is_none());
    assert!(system.disk_io().is_none());
    assert!(system.mounts().is_none());
    system.refresh_memory().unwrap();
    assert!(system.memory().unwrap().counters.total_bytes > 0);
    assert!(system.cpu().is_none());
    let memory = *system.memory().unwrap();
    system.refresh_cpu().unwrap();
    assert_eq!(*system.memory().unwrap(), memory);
    assert!(!system.cpu().unwrap().cores().is_empty());
    let pid = std::process::id();
    system.refresh_process(pid).unwrap();
    assert_eq!(system.processes().count(), 1);
    assert_eq!(system.process(pid).unwrap().id.pid, pid);
    assert_eq!(system.process_table().unwrap().len(), 1);
}

#[test]
fn independent_collectors_are_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<System>();
    check::<zsys::linux::CpuCollector>();
    check::<zsys::linux::MemoryCollector>();
    check::<zsys::linux::Processes>();
    check::<zsys::linux::Networks>();
    check::<zsys::linux::DiskIoCollector>();
    check::<zsys::linux::Mounts>();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(|| {
                let mut system = System::new();
                for _ in 0..25 {
                    system.refresh_memory().unwrap();
                    system.refresh_process(std::process::id()).unwrap();
                }
                system.memory().unwrap().sample.sequence
            })
        })
        .collect();
    for worker in workers {
        assert_eq!(worker.join().unwrap(), 25);
    }
}
