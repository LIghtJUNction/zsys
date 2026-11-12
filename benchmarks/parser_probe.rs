#![allow(dead_code)]
// Pure parser microbenchmark: rustc --edition=2024 -O benchmarks/parser_probe.rs -o /tmp/zsys-parsers
#[path = "../src/cpu.rs"]
mod cpu;
#[path = "../src/memory.rs"]
mod memory;
#[path = "../src/linux/parse.rs"]
mod parse;
#[path = "../src/process.rs"]
mod process;
use std::{hint::black_box, time::Instant};
fn main() {
    let mem = std::fs::read("/proc/meminfo").unwrap();
    let stat = std::fs::read("/proc/stat").unwrap();
    let pid = std::fs::read("/proc/self/stat").unwrap();
    for repeat in 0..7 {
        let n = 200_000;
        let t = Instant::now();
        for _ in 0..n {
            black_box(parse::parse_meminfo(black_box(&mem)).unwrap());
        }
        println!("memory,{repeat},{}", t.elapsed().as_nanos() / n);
        let t = Instant::now();
        for _ in 0..n {
            black_box(parse::parse_cpu_stat(black_box(&stat)).unwrap());
        }
        println!("cpu,{repeat},{}", t.elapsed().as_nanos() / n);
        let t = Instant::now();
        for _ in 0..n {
            black_box(parse::process_stat(black_box(&pid)).unwrap());
        }
        println!("process,{repeat},{}", t.elapsed().as_nanos() / n);
    }
}
