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
    /// Opens every port named in the routing. A missing port is reported and its
    /// instruments stay silent; the engine keeps running.
    pub fn open(routing: &Routing) -> (Outputs, Vec<String>) {
        let mut warnings = Vec::new();
        let mut opened: Vec<(String, midir::MidiOutputConnection)> = Vec::new();
        let mut tracks = [None; TRACKS];
        for (t, inst) in routing.instruments.iter().take(TRACKS).enumerate() {
            let want = inst.port.to_lowercase();
            if let Some(i) = opened.iter().position(|(n, _)| n.to_lowercase().contains(&want)) {
                tracks[t] = Some((i, inst.channel.clamp(1, 16)));
                continue;
            }
            let Ok(out) = midir::MidiOutput::new("seq-engine") else {
                warnings.push("MIDI system unavailable".into());
                break;
            };
            let port = out.ports().into_iter().find(|p| {
                out.port_name(p).map(|n| n.to_lowercase().contains(&want)).unwrap_or(false)
            });
            match port {
                None => warnings.push(format!("{}: no port matching '{}'", inst.name, inst.port)),
                Some(p) => {
                    let name = out.port_name(&p).unwrap_or_default();
                    match out.connect(&p, "seq-engine") {
                        Ok(c) => {
                            tracks[t] = Some((opened.len(), inst.channel.clamp(1, 16)));
                            opened.push((name, c));
                        }
                        Err(e) => warnings.push(format!("{}: cannot open '{}': {e}", inst.name, name)),
                    }
                }
            }
        }
        let conns = opened.into_iter().map(|(_, c)| c).collect();
        (Outputs { conns, tracks }, warnings)
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
