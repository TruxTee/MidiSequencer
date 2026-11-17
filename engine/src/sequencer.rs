//! Pure timing core: no OS calls, no allocation. Time is nanoseconds on any monotonic clock.

use crate::pattern::{Pattern, STEPS, TRACKS};

pub const PPQN: u64 = 24;
pub const TICKS_PER_STEP: u64 = 6; // 16th notes
pub const GATE_TICKS: u64 = 3; // notes last half a step
pub const BPM_MILLI_MIN: u32 = 20_000;
pub const BPM_MILLI_MAX: u32 = 300_000;
/// Upper bound on ticks handled per advance() call, so a stalled caller cannot spin forever.
const MAX_TICKS_PER_ADVANCE: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Start,
    Stop,
    Clock,
    NoteOn { track: u8, note: u8, velocity: u8 },
    NoteOff { track: u8, note: u8 },
}

pub struct Sequencer {
    pub pattern: Pattern,
    bpm_milli: u32,
    running: bool,
    // Tick times are computed from an anchor, never accumulated, so tempo math cannot drift.
    anchor_ns: u64,
    anchor_tick: u64,
    next_tick: u64,
    sounding: [Option<u8>; TRACKS],
}

impl Sequencer {
    pub fn new(pattern: Pattern, bpm_milli: u32) -> Self {
        Sequencer {
            pattern,
            bpm_milli: bpm_milli.clamp(BPM_MILLI_MIN, BPM_MILLI_MAX),
            running: false,
            anchor_ns: 0,
            anchor_tick: 0,
            next_tick: 0,
            sounding: [None; TRACKS],
        }
    }

    pub fn bpm_milli(&self) -> u32 {
        self.bpm_milli
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    fn tick_time(&self, tick: u64) -> u64 {
        let ticks = (tick - self.anchor_tick) as u128;
        let ns = ticks * 60_000_000_000_000u128 / (self.bpm_milli as u128 * PPQN as u128);
        self.anchor_ns + ns as u64
    }

    pub fn start(&mut self, now_ns: u64, out: &mut impl FnMut(u64, Event)) {
        if self.running {
            return;
        }
        self.running = true;
        self.anchor_ns = now_ns;
        self.anchor_tick = 0;
        self.next_tick = 0;
        out(now_ns, Event::Start);
    }

    pub fn stop(&mut self, now_ns: u64, out: &mut impl FnMut(u64, Event)) {
        if !self.running {
            return;
        }
        self.running = false;
        self.release_all(now_ns, out);
        out(now_ns, Event::Stop);
    }

    /// Takes effect from the next tick; ticks already played keep their times.
    pub fn set_tempo(&mut self, bpm_milli: u32) {
        let bpm_milli = bpm_milli.clamp(BPM_MILLI_MIN, BPM_MILLI_MAX);
        if self.running {
            self.anchor_ns = self.tick_time(self.next_tick);
            self.anchor_tick = self.next_tick;
        }
        self.bpm_milli = bpm_milli;
    }

    /// When the next event is due, or None while stopped.
    pub fn next_due(&self) -> Option<u64> {
        self.running.then(|| self.tick_time(self.next_tick))
    }

    /// Step index that is playing (or last played), 0..STEPS.
    pub fn current_step(&self) -> usize {
        (self.next_tick.saturating_sub(1) / TICKS_PER_STEP) as usize % STEPS
    }

    /// Emits every event due at or before `now_ns`, each with its scheduled time.
    pub fn advance(&mut self, now_ns: u64, out: &mut impl FnMut(u64, Event)) {
        for _ in 0..MAX_TICKS_PER_ADVANCE {
            if !self.running {
                return;
            }
            let at = self.tick_time(self.next_tick);
            if at > now_ns {
                return;
            }
            let tick = self.next_tick;
            self.next_tick += 1;

            out(at, Event::Clock);
            match tick % TICKS_PER_STEP {
                GATE_TICKS => self.release_all(at, out),
                0 => {
                    self.release_all(at, out);
                    let step = (tick / TICKS_PER_STEP) as usize % STEPS;
                    for t in 0..TRACKS {
                        let s = self.pattern.step(t, step);
                        if s.on {
                            out(at, Event::NoteOn { track: t as u8, note: s.note, velocity: s.velocity });
                            self.sounding[t] = Some(s.note);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Releases the note that was actually started, even if the step was edited meanwhile.
    fn release_all(&mut self, at: u64, out: &mut impl FnMut(u64, Event)) {
        for t in 0..TRACKS {
            if let Some(note) = self.sounding[t].take() {
                out(at, Event::NoteOff { track: t as u8, note });
            }
        }
    }
}
