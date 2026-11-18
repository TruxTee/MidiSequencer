//! What the controls mean (Milestone 1): track buttons pick the track, PITCH is the active
//! parameter, encoders set step notes, encoder push toggles a step, start/stop runs transport.
//! Pure logic: no I/O, no allocation.

use crate::control::{Control, Feedback, Transport, ENCODERS, PARAM_BUTTONS, TRACK_BUTTONS};
use crate::pattern::TRACKS;
use crate::sequencer::{Event, Sequencer};

pub const PARAM_PITCH: u8 = 0; // B5

pub struct Surface {
    pub track: u8,
    pub param: u8,
}

/// Whether handling a control changed something that must be saved.
#[must_use]
pub struct Changed(pub bool);

impl Default for Surface {
    fn default() -> Self {
        Surface { track: 0, param: PARAM_PITCH }
    }
}

impl Surface {
    pub fn handle(
        &mut self,
        c: Control,
        seq: &mut Sequencer,
        now_ns: u64,
        events: &mut impl FnMut(u64, Event),
        fb: &mut impl FnMut(Feedback),
    ) -> Changed {
        match c {
            Control::TrackButton { index, pressed: true } if (index as usize) < TRACKS => {
                self.track = index;
                self.refresh(seq, fb);
            }
            Control::ParamButton { index, pressed: true } if (index as usize) < PARAM_BUTTONS => {
                self.param = index;
                self.refresh(seq, fb);
            }
            Control::EncoderTurn { index, delta } if self.param == PARAM_PITCH && (index as usize) < ENCODERS => {
                let (t, s) = (self.track as usize, index as usize);
                let note = seq.pattern.tracks[t].steps[s].note as i32 + delta as i32;
                seq.pattern.set_note(t, s, note);
                fb(Feedback::Ring { index, value: seq.pattern.tracks[t].steps[s].note });
                return Changed(true);
            }
            Control::EncoderPush { index, pressed: true } if (index as usize) < ENCODERS => {
                let step = &mut seq.pattern.tracks[self.track as usize].steps[index as usize];
                step.on = !step.on;
                fb(Feedback::EncoderLed { index, on: step.on });
                return Changed(true);
            }
            // Turns that change nothing still get the true value back, so a controller that
            // moved its ring ahead of the engine is corrected.
            Control::EncoderTurn { index, .. } if (index as usize) < ENCODERS => {
                fb(Feedback::Ring { index, value: self.ring_value(seq, index as usize) });
            }
            Control::Tempo { delta } => {
                let bpm = (seq.bpm_milli() as i32 + 500) / 1000 + delta as i32;
                seq.set_tempo(bpm.max(0) as u32 * 1000);
                fb(Feedback::Tempo { bpm: bpm_of(seq) });
                return Changed(true);
            }
            Control::Transport { which: Transport::StartStop, pressed: true }
            | Control::Footswitch { index: 0, pressed: true } => {
                if seq.is_running() {
                    seq.stop(now_ns, events);
                } else {
                    seq.start(now_ns, events);
                }
                fb(Feedback::TransportLed { which: Transport::StartStop, on: seq.is_running() });
            }
            _ => {}
        }
        Changed(false)
    }

    fn ring_value(&self, seq: &Sequencer, step: usize) -> u8 {
        if self.param == PARAM_PITCH { seq.pattern.tracks[self.track as usize].steps[step].note } else { 0 }
    }

    /// Sends the full picture: rings and step lights of the selected track, selected buttons.
    pub fn refresh(&self, seq: &Sequencer, fb: &mut impl FnMut(Feedback)) {
        let steps = &seq.pattern.tracks[self.track as usize].steps;
        for i in 0..ENCODERS {
            fb(Feedback::Ring { index: i as u8, value: self.ring_value(seq, i) });
            fb(Feedback::EncoderLed { index: i as u8, on: steps[i].on });
        }
        for i in 0..PARAM_BUTTONS as u8 {
            fb(Feedback::ParamLed { index: i, on: i == self.param });
        }
        for i in 0..TRACK_BUTTONS as u8 {
            fb(Feedback::TrackLed { index: i, on: i == self.track });
        }
        fb(Feedback::TransportLed { which: Transport::StartStop, on: seq.is_running() });
        fb(Feedback::Tempo { bpm: bpm_of(seq) });
    }
}

fn bpm_of(seq: &Sequencer) -> u16 {
    ((seq.bpm_milli() + 500) / 1000) as u16
}
