//! The engine process: plays the pattern to the routed MIDI ports, takes edits from the
//! control surface, sends feedback to it, and autosaves. Ports that drop off and come back
//! (network sessions, USB devices) are reconnected while it runs.
//! Usage: seq-engine [--state P] [--routing P] [--profile P] [--seconds N] [--play] [--print]

use seq_engine::control::{Feedback, Profile, Transport};
use seq_engine::midi::{Outputs, Routing};
use seq_engine::ports::{wiring, Wiring};
use seq_engine::sequencer::{Event, Sequencer};
use seq_engine::state::{load, save_atomic, Autosave, State};
use seq_engine::surface::Surface;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::{Duration, Instant};

const AUTOSAVE_QUIET_NS: u64 = 3_000_000_000;
const AUTOSAVE_MAX_WAIT_NS: u64 = 10_000_000_000;
/// Longest wait while idle, so autosave deadlines and port changes are still checked.
const IDLE_WAIT_NS: u64 = 250_000_000;
/// How often the watcher lists ports. ALSA reuses port numbers, so a port that vanishes and
/// returns faster than this can go unnoticed; network sessions take seconds to come back.
const PORT_SCAN: Duration = Duration::from_millis(500);

type ControlMsg = ([u8; 3], usize);

fn read_json<T: serde::de::DeserializeOwned>(path: &PathBuf) -> T {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_slice(&data).unwrap_or_else(|e| panic!("bad file {}: {e}", path.display()))
}

fn port_names<T: midir::MidiIO>(io: &T) -> Vec<String> {
    io.ports().iter().filter_map(|p| io.port_name(p).ok()).collect()
}

/// Lists ports off the timing thread and reports the wanted wiring whenever it changes.
fn spawn_watcher(profile: Profile, routing: Routing, tx: SyncSender<Wiring>) {
    std::thread::spawn(move || {
        let (Ok(inp), Ok(out)) = (midir::MidiInput::new("seq-engine-scan"), midir::MidiOutput::new("seq-engine-scan"))
        else {
            eprintln!("port watcher: MIDI system unavailable");
            return;
        };
        let mut last: Option<Wiring> = None;
        loop {
            let w = wiring(&port_names(&inp), &port_names(&out), &profile, &routing);
            if last.as_ref() != Some(&w) {
                if tx.send(w.clone()).is_err() {
                    return;
                }
                last = Some(w);
            }
            std::thread::sleep(PORT_SCAN);
        }
    });
}

fn connect_input(name: &str, tx: SyncSender<ControlMsg>) -> Option<midir::MidiInputConnection<()>> {
    let inp = midir::MidiInput::new("seq-engine-in").ok()?;
    let port = inp.ports().into_iter().find(|p| inp.port_name(p).ok().as_deref() == Some(name))?;
    // Controller input arrives on midir's thread; a bounded, preallocated queue hands it over.
    inp.connect(&port, "seq-engine-in", move |_, m, _| {
        if m.len() <= 3 {
            let mut b = [0u8; 3];
            b[..m.len()].copy_from_slice(m);
            let _ = tx.try_send((b, m.len()));
        }
    }, ())
    .ok()
}

fn connect_output(client: &str, name: &str) -> Option<midir::MidiOutputConnection> {
    let out = midir::MidiOutput::new(client).ok()?;
    let port = out.ports().into_iter().find(|p| out.port_name(p).ok().as_deref() == Some(name))?;
    out.connect(&port, client).ok()
}

/// Live connections, swapped by `apply` when the wiring changes.
struct Io {
    wiring: Wiring,
    outputs: Outputs,
    input: Option<midir::MidiInputConnection<()>>,
    feedback: Option<midir::MidiOutputConnection>,
}

impl Io {
    /// Reconnects the parts whose port changed. Returns true if the feedback port is new, so
    /// the caller resends the whole surface.
    fn apply(&mut self, w: Wiring, routing: &Routing, in_tx: &SyncSender<ControlMsg>) -> bool {
        let label = |p: &Option<String>| p.clone().unwrap_or_else(|| "none".into());
        if w.outputs != self.wiring.outputs {
            let (outputs, warnings) = Outputs::open(routing, &w.outputs);
            self.outputs = outputs;
            // Notes left hanging while the port was gone, or by a crash, stop here.
            self.outputs.all_notes_off();
            for warning in warnings {
                println!("warning: {warning}");
            }
            println!("outputs: {:?}", w.outputs);
        }
        if w.controller_in != self.wiring.controller_in {
            self.input = None; // close the old one before opening the same name again
            self.input = w.controller_in.as_deref().and_then(|n| connect_input(n, in_tx.clone()));
            println!("controller in: {}", label(&w.controller_in));
        }
        let new_feedback = w.feedback != self.wiring.feedback;
        if new_feedback {
            self.feedback = None;
            self.feedback = w.feedback.as_deref().and_then(|n| connect_output("seq-engine-fb", n));
            println!("controller feedback: {}", label(&w.feedback));
        }
        let refresh = (new_feedback || w.controller_in != self.wiring.controller_in) && self.feedback.is_some();
        self.wiring = w;
        refresh
    }
}

/// Newest wiring waiting in the queue, if any.
fn latest(rx: &Receiver<Wiring>) -> Option<Wiring> {
    rx.try_iter().last()
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

    let (in_tx, in_rx) = sync_channel::<ControlMsg>(256);
    let (wiring_tx, wiring_rx) = sync_channel::<Wiring>(4);
    spawn_watcher(profile.clone(), routing.clone(), wiring_tx);

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

    // Every field starts disconnected, so the first wiring report connects all that exists.
    let mut io = Io {
        wiring: Wiring { controller_in: None, feedback: None, outputs: Default::default() },
        outputs: Outputs::none(),
        input: None,
        feedback: None,
    };
    let first = wiring_rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let mut needs_refresh = io.apply(first, &routing, &in_tx);
    if io.wiring.controller_in.is_none() {
        println!("warning: no controller input matching '{}' yet; waiting for it", profile.port);
    }

    let t0 = Instant::now();
    let now = || t0.elapsed().as_nanos() as u64;
    let mut seq = Sequencer::new(state.pattern.clone(), state.bpm_milli);
    let mut surface = Surface::default();
    let mut autosave = Autosave::new(AUTOSAVE_QUIET_NS, AUTOSAVE_MAX_WAIT_NS);
    let (mut sent, mut worst_late_ns) = (0u64, 0u64);
    let end = if seconds == 0 { u64::MAX } else { seconds * 1_000_000_000 };
    let mut started = !play;

    while now() < end {
        if let Some(w) = latest(&wiring_rx) {
            needs_refresh |= io.apply(w, &routing, &in_tx);
        }
        // Built per pass so `io` can be reconnected between passes.
        let Io { outputs, feedback, .. } = &mut io;
        let mut emit = |at: u64, e: Event| {
            worst_late_ns = worst_late_ns.max(now().saturating_sub(at));
            outputs.send(e);
            sent += 1;
            if print && e != Event::Clock {
                println!("{:>9.3} ms  {:?}", at as f64 / 1e6, e);
            }
        };
        let mut fb = |f: Feedback| {
            if let Some(c) = feedback.as_mut() {
                let _ = c.send(&profile.encode(f));
            }
        };
        if needs_refresh {
            surface.refresh(&seq, &mut fb);
            needs_refresh = false;
        }
        if !started {
            seq.start(now(), &mut emit);
            fb(Feedback::TransportLed { which: Transport::StartStop, on: true });
            started = true;
        }

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
            Err(RecvTimeoutError::Disconnected) => unreachable!("main holds a sender"),
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

    seq.stop(now(), &mut |at, e| {
        worst_late_ns = worst_late_ns.max(now().saturating_sub(at));
        io.outputs.send(e);
        sent += 1;
    });
    println!("events sent: {sent}, worst lateness: {:.3} ms", worst_late_ns as f64 / 1e6);
    let _ = save_tx.send(State { pattern: seq.pattern.clone(), bpm_milli: seq.bpm_milli() });
    drop(save_tx);
    let _ = saver.join();
}
