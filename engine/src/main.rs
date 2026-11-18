//! Plays the saved pattern in real time to the MIDI ports named in the routing file.
//! Usage: seq-engine [--state P] [--routing P] [--seconds N (0 = forever)] [--print]

use seq_engine::midi::{Outputs, Routing};
use seq_engine::sequencer::{Event, Sequencer};
use seq_engine::state::{load, save_atomic, State};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let (mut state_path, mut routing_path, mut seconds, mut print) =
        (PathBuf::from("state.json"), PathBuf::from("routing.json"), 0u64, false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--state" => state_path = args.next().expect("--state needs a path").into(),
            "--routing" => routing_path = args.next().expect("--routing needs a path").into(),
            "--seconds" => seconds = args.next().and_then(|s| s.parse().ok()).expect("--seconds needs a number"),
            "--print" => print = true,
            other => panic!("unknown argument {other}"),
        }
    }

    let (state, outcome) = load(&state_path);
    println!("state {}: {:?}, {} BPM", state_path.display(), outcome, state.bpm_milli as f64 / 1000.0);

    let routing: Routing = serde_json::from_slice(
        &std::fs::read(&routing_path).unwrap_or_else(|e| panic!("cannot read {}: {e}", routing_path.display())),
    )
    .unwrap_or_else(|e| panic!("bad routing file {}: {e}", routing_path.display()));
    let (mut outputs, warnings) = Outputs::open(&routing);
    for w in &warnings {
        println!("warning: {w}");
    }

    let t0 = Instant::now();
    let now = || t0.elapsed().as_nanos() as u64;
    let mut seq = Sequencer::new(state.pattern.clone(), state.bpm_milli);
    let (mut sent, mut worst_late_ns) = (0u64, 0u64);
    let mut emit = |at: u64, e: Event| {
        worst_late_ns = worst_late_ns.max(now().saturating_sub(at));
        outputs.send(e);
        sent += 1;
        if print && e != Event::Clock {
            println!("{:>9.3} ms  {:?}", at as f64 / 1e6, e);
        }
    };

    seq.start(now(), &mut emit);
    let end = if seconds == 0 { u64::MAX } else { seconds * 1_000_000_000 };
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
    println!("events sent: {sent}, worst lateness: {:.3} ms", worst_late_ns as f64 / 1e6);

    let bpm_milli = seq.bpm_milli();
    save_atomic(&state_path, &State { pattern: seq.pattern, bpm_milli }).expect("save failed");
}
