//! Device profiles: translate a controller's raw MIDI into surface controls and back.
//! The layout is the BCR32's assignable set; the profile says which MIDI each control uses.

use serde::Deserialize;

pub const ENCODERS: usize = 32;
pub const PARAM_BUTTONS: usize = 16;
pub const TRACK_BUTTONS: usize = 4;
pub const TRANSPORT_BUTTONS: usize = 4;
pub const FOOTSWITCHES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Rec,
    Mixer,
    Reset,
    StartStop,
}

const TRANSPORT_ORDER: [Transport; TRANSPORT_BUTTONS] =
    [Transport::Rec, Transport::Mixer, Transport::Reset, Transport::StartStop];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    EncoderTurn { index: u8, delta: i8 },
    EncoderPush { index: u8, pressed: bool },
    ParamButton { index: u8, pressed: bool },
    TrackButton { index: u8, pressed: bool },
    Transport { which: Transport, pressed: bool },
    Footswitch { index: u8, pressed: bool },
    /// Relative tempo change in whole BPM.
    Tempo { delta: i8 },
}

/// What the engine shows on the device: ring values and button lights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feedback {
    Ring { index: u8, value: u8 },
    EncoderLed { index: u8, on: bool },
    ParamLed { index: u8, on: bool },
    TrackLed { index: u8, on: bool },
    TransportLed { which: Transport, on: bool },
    /// Current tempo in whole BPM.
    Tempo { bpm: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncoderMode {
    /// 1..=63 clockwise steps, 65..=127 counter-clockwise (128 - value) steps.
    RelativeTwos,
    /// 64 = no change; above is clockwise, below counter-clockwise.
    RelativeOffset64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CcMap {
    pub encoder_turn: u8,
    pub encoder_push: u8,
    pub param_button: u8,
    pub track_button: u8,
    pub transport: u8,
    pub footswitch: u8,
    /// Relative tempo control; devices without one leave it out.
    #[serde(default)]
    pub tempo: Option<u8>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Profile {
    pub name: String,
    /// Case-insensitive substring of the controller's input port name.
    pub port: String,
    /// Substring of the port feedback goes to; defaults to `port`.
    #[serde(default)]
    pub feedback_port: Option<String>,
    pub channel: u8,
    pub encoder_mode: EncoderMode,
    /// First CC of each control group; groups occupy consecutive CCs.
    pub cc: CcMap,
}

fn in_group(cc: u8, first: u8, count: usize) -> Option<u8> {
    let i = cc.checked_sub(first)?;
    ((i as usize) < count).then_some(i)
}

impl Profile {
    fn status(&self) -> u8 {
        0xB0 | (self.channel.clamp(1, 16) - 1)
    }

    /// Returns None for anything that is not one of this device's controls.
    pub fn decode(&self, msg: &[u8]) -> Option<Control> {
        let [status, cc, value] = *msg else { return None };
        if status != self.status() {
            return None;
        }
        let c = &self.cc;
        let pressed = value >= 64;
        if let Some(index) = in_group(cc, c.encoder_turn, ENCODERS) {
            let delta = self.relative(value);
            return (delta != 0).then_some(Control::EncoderTurn { index, delta });
        }
        if c.tempo == Some(cc) {
            let delta = self.relative(value);
            return (delta != 0).then_some(Control::Tempo { delta });
        }
        if let Some(index) = in_group(cc, c.encoder_push, ENCODERS) {
            return Some(Control::EncoderPush { index, pressed });
        }
        if let Some(index) = in_group(cc, c.param_button, PARAM_BUTTONS) {
            return Some(Control::ParamButton { index, pressed });
        }
        if let Some(index) = in_group(cc, c.track_button, TRACK_BUTTONS) {
            return Some(Control::TrackButton { index, pressed });
        }
        if let Some(i) = in_group(cc, c.transport, TRANSPORT_BUTTONS) {
            return Some(Control::Transport { which: TRANSPORT_ORDER[i as usize], pressed });
        }
        if let Some(index) = in_group(cc, c.footswitch, FOOTSWITCHES) {
            return Some(Control::Footswitch { index, pressed });
        }
        None
    }

    fn relative(&self, value: u8) -> i8 {
        match self.encoder_mode {
            EncoderMode::RelativeTwos => match value {
                1..=63 => value as i8,
                65..=127 => -((128 - value as i16) as i8),
                _ => 0,
            },
            EncoderMode::RelativeOffset64 => (value as i16 - 64) as i8,
        }
    }

    /// Feedback goes out on the same CC as the control it belongs to. Tempo goes out as one
    /// pitch-bend message (14 bits) so a display never shows half of an update.
    pub fn encode(&self, f: Feedback) -> [u8; 3] {
        if let Feedback::Tempo { bpm } = f {
            let bpm = bpm.min(0x3FFF);
            return [0xE0 | (self.channel.clamp(1, 16) - 1), (bpm & 0x7F) as u8, (bpm >> 7) as u8];
        }
        let c = &self.cc;
        let lit = |on: bool| if on { 127 } else { 0 };
        let (cc, value) = match f {
            Feedback::Ring { index, value } => (c.encoder_turn + index, value.min(127)),
            Feedback::EncoderLed { index, on } => (c.encoder_push + index, lit(on)),
            Feedback::ParamLed { index, on } => (c.param_button + index, lit(on)),
            Feedback::TrackLed { index, on } => (c.track_button + index, lit(on)),
            Feedback::TransportLed { which, on } => {
                let i = TRANSPORT_ORDER.iter().position(|t| *t == which).unwrap_or(0) as u8;
                (c.transport + i, lit(on))
            }
            Feedback::Tempo { .. } => unreachable!("handled above"),
        };
        [self.status(), cc & 0x7F, value]
    }
}

impl Profile {
    /// Rejects profiles whose control groups run past CC 127 or overlap.
    pub fn validate(&self) -> Result<(), String> {
        let c = &self.cc;
        let mut groups = vec![
            ("encoder_turn", c.encoder_turn, ENCODERS),
            ("encoder_push", c.encoder_push, ENCODERS),
            ("param_button", c.param_button, PARAM_BUTTONS),
            ("track_button", c.track_button, TRACK_BUTTONS),
            ("transport", c.transport, TRANSPORT_BUTTONS),
            ("footswitch", c.footswitch, FOOTSWITCHES),
        ];
        if let Some(t) = c.tempo {
            groups.push(("tempo", t, 1));
        }
        for &(name, first, count) in &groups {
            if first as usize + count > 128 {
                return Err(format!("{name}: CCs {first}..{} run past 127", first as usize + count - 1));
            }
        }
        groups.sort_by_key(|g| g.1);
        for w in groups.windows(2) {
            if w[0].1 as usize + w[0].2 > w[1].1 as usize {
                return Err(format!("{} overlaps {}", w[0].0, w[1].0));
            }
        }
        Ok(())
    }
}
