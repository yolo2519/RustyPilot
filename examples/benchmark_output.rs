//! Terminal output benchmark tool
//!
//! Similar to `yes` but with performance statistics.
//! Usage: cargo run --example benchmark_output
//! Press Ctrl+C to see statistics.

use std::time::Instant;

fn main() {
    let start_time = Instant::now();


    eprintln!("Starting benchmark... (Press Ctrl+C to stop and see stats)");
    eprintln!("Output pattern: 'Line NNNNNNNN'");
    eprintln!();

    // Main output loop
    let mut line_num = 0u64;
    while (Instant::now() - start_time).as_secs_f64() < 5. {
        // Write to buffer
        println!("Line {:08}", line_num);
        line_num += 1;
    }
    let end_time = Instant::now();
    let elapsed = (end_time - start_time).as_secs_f64();
    let tps = line_num as f64 / elapsed;
    // Print to stderr so it doesn't interfere with stdout capture
    eprintln!("\n\n========== Benchmark Results ==========");
    eprintln!("Total lines:     {:>12}", format_number(line_num));
    eprintln!("Elapsed time:    {:>12.2} seconds", elapsed);
    if tps > 1_000_000. {
        eprintln!("Throughput:      {:>12.2}M lines/sec", tps / 1_000_000.);
    } else if tps > 1000. {
        eprintln!("Throughput:      {:>12.2}k lines/sec", tps / 1000.);
    } else {
        eprintln!("Throughput:      {:>12.2} lines/sec", tps);
    }
    eprintln!("========================================\n");

    // Buffers will be automatically flushed on normal program exit
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}
