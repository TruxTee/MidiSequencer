//! MIDI byte encoding (pure) and routing of engine tracks to output ports.

use crate::pattern::TRACKS;
use crate::sequencer::Event;
use serde::Deserialize;

/// Encodes an event for one MIDI channel (1..=16, clamped). Returns the bytes and their count.
pub fn encode(e: Event, channel: u8) -> ([u8; 3], usize) {
    let ch = channel.clamp(1, 16) - 1;
    match e {
        Event::NoteOn { note, velocity, .. } => ([0x90 | ch, note & 0x7F, velocity & 0x7F], 3),
        Event::NoteOff { note, .. } => ([0x80 | ch, note & 0x7F, 0], 3),
        Event::Clock => ([0xF8, 0, 0], 1),
        Event::Start => ([0xFA, 0, 0], 1),
        Event::Stop => ([0xFC, 0, 0], 1),
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Instrument {
    pub name: String,
    /// Matched as a case-insensitive substring of the port name.
    pub port: String,
    pub channel: u8,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Routing {
    /// One entry per track, in track order.
    pub instruments: Vec<Instrument>,
}

pub struct Outputs {
    conns: Vec<midir::MidiOutputConnection>,
    tracks: [Option<(usize, u8)>; TRACKS],
}

impl Outputs {
    /// Opens the resolved port of each track (see `ports::wiring`); tracks sharing a port share
    /// one connection. A missing port is reported and its track stays silent.
    pub fn open(routing: &Routing, ports: &[Option<String>; TRACKS]) -> (Outputs, Vec<String>) {
        let mut warnings = Vec::new();
        let mut opened: Vec<(String, midir::MidiOutputConnection)> = Vec::new();
        let mut tracks = [None; TRACKS];
        for (t, inst) in routing.instruments.iter().take(TRACKS).enumerate() {
            let Some(name) = &ports[t] else {
                warnings.push(format!("{}: no port matching '{}'", inst.name, inst.port));
                continue;
            };
            let channel = inst.channel.clamp(1, 16);
            if let Some(i) = opened.iter().position(|(n, _)| n == name) {
                tracks[t] = Some((i, channel));
                continue;
            }
            let Ok(out) = midir::MidiOutput::new("seq-engine") else {
                warnings.push("MIDI system unavailable".into());
                break;
            };
            let Some(port) = out.ports().into_iter().find(|p| out.port_name(p).ok().as_ref() == Some(name)) else {
                warnings.push(format!("{}: port '{name}' vanished", inst.name));
                continue;
            };
            match out.connect(&port, "seq-engine") {
                Ok(c) => {
                    tracks[t] = Some((opened.len(), channel));
                    opened.push((name.clone(), c));
                }
                Err(e) => warnings.push(format!("{}: cannot open '{name}': {e}", inst.name)),
            }
        }
        let conns = opened.into_iter().map(|(_, c)| c).collect();
        (Outputs { conns, tracks }, warnings)
    }

    /// No ports: everything is dropped until the next `open`.
    pub fn none() -> Outputs {
        Outputs { conns: Vec::new(), tracks: [None; TRACKS] }
    }

    /// All Notes Off (CC 123) on every routed channel, so notes left hanging by a crash stop.
    pub fn all_notes_off(&mut self) {
        for (i, ch) in self.tracks.iter().flatten() {
            let _ = self.conns[*i].send(&[0xB0 | (ch - 1), 123, 0]);
        }
    }

    pub fn send(&mut self, e: Event) {
        match e {
            Event::NoteOn { track, .. } | Event::NoteOff { track, .. } => {
                if let Some(Some((i, ch))) = self.tracks.get(track as usize) {
                    let (b, n) = encode(e, *ch);
                    let _ = self.conns[*i].send(&b[..n]);
                }
            }
            // Transport and clock go to every open port.
            _ => {
                let (b, n) = encode(e, 1);
                for c in self.conns.iter_mut() {
                    let _ = c.send(&b[..n]);
                }
            }
        }
    }
}
