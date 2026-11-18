//! Dev check: listens on an input port and reports what arrived and how evenly.
//! Usage: cargo run --example monitor -- <port substring> <seconds>
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    let mut a = std::env::args().skip(1);
    let want = a.next().unwrap_or("loopMIDI".into()).to_lowercase();
    let secs: u64 = a.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let inp = midir::MidiInput::new("seq-monitor").unwrap();
    let port = inp.ports().into_iter()
        .find(|p| inp.port_name(p).unwrap_or_default().to_lowercase().contains(&want))
        .expect("no matching input port");
    println!("listening on {}", inp.port_name(&port).unwrap());
    let t0 = Instant::now();
    let log: Arc<Mutex<Vec<(f64, Vec<u8>)>>> = Arc::default();
    let l2 = log.clone();
    let mut inp = inp;
    inp.ignore(midir::Ignore::None);
    let _c = inp.connect(&port, "mon", move |_, m, _| {
        l2.lock().unwrap().push((t0.elapsed().as_secs_f64() * 1000.0, m.to_vec()))
    }, ()).unwrap();
    std::thread::sleep(Duration::from_secs(secs));
    let log = log.lock().unwrap();
    let count = |s: u8| log.iter().filter(|(_, m)| m[0] & 0xF0 == s || m[0] == s).count();
    println!("start {} stop {} clock {} note-on {} note-off {}",
        count(0xFA), count(0xFC), count(0xF8), count(0x90), count(0x80));
    let mut seq: Vec<(u8, usize)> = Vec::new();
    for (_, m) in log.iter().filter(|(_, m)| m[0] & 0xF0 == 0x90) {
        match seq.last_mut() { Some((n, c)) if *n == m[1] => *c += 1, _ => seq.push((m[1], 1)) }
    }
    println!("note-on sequence (note x count): {:?}", seq);
    let ons: Vec<f64> = log.iter().filter(|(_, m)| m[0] & 0xF0 == 0x90).map(|(t, _)| *t).collect();
    let gaps: Vec<f64> = ons.windows(2).map(|w| w[1] - w[0]).collect();
    if !gaps.is_empty() {
        let (min, max) = gaps.iter().fold((f64::MAX, 0f64), |(a, b), &g| (a.min(g), b.max(g)));
        println!("note-on spacing: min {min:.3} ms, max {max:.3} ms");
    }
}
