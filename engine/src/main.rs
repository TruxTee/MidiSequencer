//! Step 1 smoke run: plays the saved pattern in real time and prints note events.
//! MIDI output replaces the printing in step 2.

use seq_engine::sequencer::{Event, Sequencer};
use seq_engine::state::{load, save_atomic};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(args.next().unwrap_or_else(|| "state.json".into()));
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2);

    let (state, outcome) = load(&path);
    println!("state {}: {:?}, {} BPM", path.display(), outcome, state.bpm_milli as f64 / 1000.0);

    let t0 = Instant::now();
    let now = || t0.elapsed().as_nanos() as u64;
    let mut seq = Sequencer::new(state.pattern.clone(), state.bpm_milli);
    let (mut clocks, mut worst_late_ns) = (0u64, 0u64);
    let mut emit = |at: u64, e: Event| {
        worst_late_ns = worst_late_ns.max(now().saturating_sub(at));
        match e {
            Event::Clock => clocks += 1,
            _ => println!("{:>9.3} ms  {:?}", at as f64 / 1e6, e),
        }
    };

    seq.start(now(), &mut emit);
    let end = seconds * 1_000_000_000;
    while let Some(due) = seq.next_due() {
        if due > end {
            break;
        }
        // Sleep to an absolute deadline: lateness never accumulates across ticks.
        let n = now();
        if due > n {
            std::thread::sleep(Duration::from_nanos(due - n));
        }
        seq.advance(now(), &mut emit);
    }
    seq.stop(now(), &mut emit);
    println!("clocks: {clocks}, worst lateness: {:.3} ms", worst_late_ns as f64 / 1e6);

    let bpm_milli = seq.bpm_milli();
    save_atomic(&path, &seq_engine::state::State { pattern: seq.pattern, bpm_milli })
        .expect("save failed");
}
