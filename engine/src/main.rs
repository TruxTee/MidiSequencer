//! The engine process: plays the pattern to the routed MIDI ports, takes edits from the
//! control surface, sends feedback to it, and autosaves.
//! Usage: seq-engine [--state P] [--routing P] [--profile P] [--seconds N] [--play] [--print]

use seq_engine::control::{Feedback, Profile};
use seq_engine::midi::{Outputs, Routing};
use seq_engine::sequencer::{Event, Sequencer};
use seq_engine::state::{load, save_atomic, Autosave, State};
use seq_engine::surface::Surface;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, RecvTimeoutError, TrySendError};
use std::time::{Duration, Instant};

const AUTOSAVE_QUIET_NS: u64 = 3_000_000_000;
const AUTOSAVE_MAX_WAIT_NS: u64 = 10_000_000_000;
/// Longest wait while idle, so autosave deadlines are still checked.
const IDLE_WAIT_NS: u64 = 250_000_000;

fn read_json<T: serde::de::DeserializeOwned>(path: &PathBuf) -> T {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_slice(&data).unwrap_or_else(|e| panic!("bad file {}: {e}", path.display()))
}

/// Exact name first (case-insensitive), then the first port containing `want`.
fn find_port<T: midir::MidiIO>(io: &T, want: &str) -> Option<T::Port> {
    let want = want.to_lowercase();
    let named: Vec<(T::Port, String)> =
        io.ports().into_iter().map(|p| { let n = io.port_name(&p).unwrap_or_default().to_lowercase(); (p, n) }).collect();
    let exact = named.iter().position(|(_, n)| *n == want);
    let i = exact.or_else(|| named.iter().position(|(_, n)| n.contains(&want)))?;
    named.into_iter().nth(i).map(|(p, _)| p)
}

fn main() {
    let mut state_path = PathBuf::from("state.json");
    let mut routing_path = PathBuf::from("routing.json");
    let mut profile_path = PathBuf::from("../profiles/pcr32.json");
    let (mut seconds, mut play, mut print) = (0u64, false, false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--state" => state_path = args.next().expect("--state needs a path").into(),
            "--routing" => routing_path = args.next().expect("--routing needs a path").into(),
            "--profile" => profile_path = args.next().expect("--profile needs a path").into(),
            "--seconds" => seconds = args.next().and_then(|s| s.parse().ok()).expect("--seconds needs a number"),
            "--play" => play = true,
            "--print" => print = true,
            other => panic!("unknown argument {other}"),
        }
    }

    let (state, outcome) = load(&state_path);
    println!("state {}: {:?}, {} BPM", state_path.display(), outcome, state.bpm_milli as f64 / 1000.0);
    let routing: Routing = read_json(&routing_path);
    let profile: Profile = read_json(&profile_path);
    profile.validate().unwrap_or_else(|e| panic!("profile {}: {e}", profile_path.display()));

    let (mut outputs, warnings) = Outputs::open(&routing);
    for w in &warnings {
        println!("warning: {w}");
    }
    outputs.all_notes_off();

    // Controller input arrives on midir's thread; a bounded, preallocated queue hands it over.
    let (in_tx, in_rx) = sync_channel::<([u8; 3], usize)>(256);
    let _input = midir::MidiInput::new("seq-engine-in").ok().and_then(|inp| {
        let port = find_port(&inp, &profile.port)?;
        let name = inp.port_name(&port).unwrap_or_default();
        let conn = inp.connect(&port, "seq-engine-in", move |_, m, _| {
            if m.len() <= 3 {
                let mut b = [0u8; 3];
                b[..m.len()].copy_from_slice(m);
                let _ = in_tx.try_send((b, m.len()));
            }
        }, ());
        println!("controller in: {name}");
        conn.ok()
    });
    if _input.is_none() {
        println!("warning: no controller input matching '{}'", profile.port);
    }
    let mut feedback_out = midir::MidiOutput::new("seq-engine-fb").ok().and_then(|out| {
        let port = find_port(&out, profile.feedback_port.as_deref().unwrap_or(&profile.port))?;
        let name = out.port_name(&port).unwrap_or_default();
        println!("controller feedback: {name}");
        out.connect(&port, "seq-engine-fb").ok()
    });

    // Saving happens off the timing thread: an SD card flush can take tens of milliseconds.
    let (save_tx, save_rx) = sync_channel::<State>(1);
    let save_path = state_path.clone();
    let saver = std::thread::spawn(move || {
        for st in save_rx {
            if let Err(e) = save_atomic(&save_path, &st) {
                eprintln!("save failed: {e}");
            }
        }
    });

    let t0 = Instant::now();
    let now = || t0.elapsed().as_nanos() as u64;
    let mut seq = Sequencer::new(state.pattern.clone(), state.bpm_milli);
    let mut surface = Surface::default();
    let mut autosave = Autosave::new(AUTOSAVE_QUIET_NS, AUTOSAVE_MAX_WAIT_NS);
    let (mut sent, mut worst_late_ns) = (0u64, 0u64);
    let mut emit = |at: u64, e: Event| {
        worst_late_ns = worst_late_ns.max(now().saturating_sub(at));
        outputs.send(e);
        sent += 1;
        if print && e != Event::Clock {
            println!("{:>9.3} ms  {:?}", at as f64 / 1e6, e);
        }
    };
    let mut fb = |f: Feedback| {
        if let Some(c) = feedback_out.as_mut() {
            let _ = c.send(&profile.encode(f));
        }
    };

    surface.refresh(&seq, &mut fb);
    if play {
        seq.start(now(), &mut emit);
        fb(Feedback::TransportLed { which: seq_engine::control::Transport::StartStop, on: true });
    }
    let end = if seconds == 0 { u64::MAX } else { seconds * 1_000_000_000 };

    while now() < end {
        // Wait for controller input or the next tick, whichever comes first.
        let n = now();
        let deadline = seq.next_due().unwrap_or(u64::MAX).min(n + IDLE_WAIT_NS).min(end);
        match in_rx.recv_timeout(Duration::from_nanos(deadline.saturating_sub(n))) {
            Ok((msg, len)) => {
                if let Some(c) = profile.decode(&msg[..len]) {
                    if print {
                        println!("{:>9.3} ms  control {:?}", now() as f64 / 1e6, c);
                    }
                    if surface.handle(c, &mut seq, now(), &mut emit, &mut fb).0 {
                        autosave.mark_dirty(now());
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => std::thread::sleep(Duration::from_nanos(deadline.saturating_sub(n))),
        }
        seq.advance(now(), &mut emit);

        if autosave.due(now()) {
            let st = State { pattern: seq.pattern.clone(), bpm_milli: seq.bpm_milli() };
            match save_tx.try_send(st) {
                Ok(()) => autosave.saved(),
                Err(TrySendError::Full(_)) => {} // saver busy: retry next pass
                Err(TrySendError::Disconnected(_)) => eprintln!("saver thread gone"),
            }
        }
    }

    seq.stop(now(), &mut emit);
    println!("events sent: {sent}, worst lateness: {:.3} ms", worst_late_ns as f64 / 1e6);
    let _ = save_tx.send(State { pattern: seq.pattern.clone(), bpm_milli: seq.bpm_milli() });
    drop(save_tx);
    let _ = saver.join();
}
