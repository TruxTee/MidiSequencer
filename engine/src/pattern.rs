use serde::{Deserialize, Serialize};

pub const TRACKS: usize = 4;
pub const STEPS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub on: bool,
    pub note: u8,
    pub velocity: u8,
}

impl Default for Step {
    fn default() -> Self {
        Step { on: false, note: 60, velocity: 100 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub steps: [Step; STEPS],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pattern {
    pub tracks: [Track; TRACKS],
}

impl Default for Pattern {
    /// Track 1 plays C4 on every step so a fresh rig makes sound; the others are silent.
    fn default() -> Self {
        let mut p = Pattern {
            tracks: std::array::from_fn(|_| Track { steps: [Step::default(); STEPS] }),
        };
        for s in p.tracks[0].steps.iter_mut() {
            s.on = true;
        }
        p
    }
}

impl Pattern {
    /// Out-of-range indices are ignored; the note is clamped to the MIDI range.
    pub fn set_note(&mut self, track: usize, step: usize, note: i32) {
        if let Some(s) = self.tracks.get_mut(track).and_then(|t| t.steps.get_mut(step)) {
            s.note = note.clamp(0, 127) as u8;
        }
    }

    pub fn step(&self, track: usize, step: usize) -> Step {
        self.tracks[track].steps[step % STEPS]
    }
}
