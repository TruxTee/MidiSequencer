//! Dev check: plays a scripted PCR32 session into "PCR32 in" and prints feedback from "PCR32 fb".
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

fn port<T: midir::MidiIO>(io: &T, want: &str) -> T::Port {
    io.ports().into_iter().find(|p| io.port_name(p).unwrap_or_default().eq_ignore_ascii_case(want))
        .unwrap_or_else(|| panic!("no port '{want}'"))
}

fn press(out: &mut midir::MidiOutputConnection, cc: u8) {
    out.send(&[0xBF, cc, 127]).unwrap();
    out.send(&[0xBF, cc, 0]).unwrap();
}

fn main() {
    let fb_log: Arc<Mutex<Vec<[u8; 3]>>> = Arc::default();
    let l2 = fb_log.clone();
    let inp = midir::MidiInput::new("fake-fb").unwrap();
    let p = port(&inp, "PCR32 fb");
    let _fb = inp.connect(&p, "fb", move |_, m, _| {
        if m.len() == 3 { l2.lock().unwrap().push([m[0], m[1], m[2]]) }
    }, ()).unwrap();
    let out = midir::MidiOutput::new("fake-pcr32").unwrap();
    let p = port(&out, "PCR32 in");
    let mut out = out.connect(&p, "pcr32").unwrap();

    sleep(Duration::from_millis(500));
    fb_log.lock().unwrap().clear(); // drop the boot refresh
    println!("press start/stop");
    press(&mut out, 88);
    sleep(Duration::from_millis(1000));
    println!("turn encoder 1 by +7");
    out.send(&[0xBF, 1, 7]).unwrap();
    sleep(Duration::from_millis(4500)); // step 1 comes round once per 32 steps (4 s at 120 BPM)
    println!("press start/stop");
    press(&mut out, 88);
    sleep(Duration::from_millis(300));
    for m in fb_log.lock().unwrap().iter() {
        println!("feedback: ch{} cc{} = {}", (m[0] & 0x0F) + 1, m[1], m[2]);
    }
}
