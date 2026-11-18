use seq_engine::control::{Control, EncoderMode, Feedback, Profile, Transport};
use seq_engine::pattern::Pattern;
use seq_engine::sequencer::{Event, Sequencer};
use seq_engine::surface::Surface;

fn profile() -> Profile {
    let p: Profile = serde_json::from_slice(&std::fs::read("../profiles/pcr32.json").unwrap()).unwrap();
    p.validate().unwrap();
    p
}

#[test]
fn pcr32_profile_decodes_its_cc_map_on_channel_16() {
    let p = profile();
    assert_eq!(p.encoder_mode, EncoderMode::RelativeTwos);
    assert_eq!(p.decode(&[0xBF, 1, 1]), Some(Control::EncoderTurn { index: 0, delta: 1 }));
    assert_eq!(p.decode(&[0xBF, 32, 127]), Some(Control::EncoderTurn { index: 31, delta: -1 }));
    assert_eq!(p.decode(&[0xBF, 5, 125]), Some(Control::EncoderTurn { index: 4, delta: -3 }));
    assert_eq!(p.decode(&[0xBF, 33, 127]), Some(Control::EncoderPush { index: 0, pressed: true }));
    assert_eq!(p.decode(&[0xBF, 64, 0]), Some(Control::EncoderPush { index: 31, pressed: false }));
    assert_eq!(p.decode(&[0xBF, 65, 127]), Some(Control::ParamButton { index: 0, pressed: true }));
    assert_eq!(p.decode(&[0xBF, 84, 127]), Some(Control::TrackButton { index: 3, pressed: true }));
    assert_eq!(p.decode(&[0xBF, 88, 127]), Some(Control::Transport { which: Transport::StartStop, pressed: true }));
    assert_eq!(p.decode(&[0xBF, 90, 127]), Some(Control::Footswitch { index: 1, pressed: true }));
}

#[test]
fn profile_ignores_other_channels_messages_and_unmapped_ccs() {
    let p = profile();
    assert_eq!(p.decode(&[0xB0, 1, 1]), None); // channel 1
    assert_eq!(p.decode(&[0x9F, 1, 1]), None); // note on
    assert_eq!(p.decode(&[0xBF, 100, 127]), None); // unmapped CC
    assert_eq!(p.decode(&[0xBF, 1, 0]), None); // zero turn
    assert_eq!(p.decode(&[0xF8]), None); // clock
}

#[test]
fn feedback_uses_the_controls_own_cc() {
    let p = profile();
    assert_eq!(p.encode(Feedback::Ring { index: 2, value: 64 }), [0xBF, 3, 64]);
    assert_eq!(p.encode(Feedback::EncoderLed { index: 0, on: true }), [0xBF, 33, 127]);
    assert_eq!(p.encode(Feedback::TrackLed { index: 1, on: false }), [0xBF, 82, 0]);
    assert_eq!(p.encode(Feedback::TransportLed { which: Transport::StartStop, on: true }), [0xBF, 88, 127]);
}

#[test]
fn validate_rejects_overflow_and_overlap() {
    let mut p = profile();
    p.cc.encoder_turn = 100; // 100..131
    assert!(p.validate().is_err());
    let mut p = profile();
    p.cc.encoder_push = 20; // overlaps encoder_turn 1..32
    assert!(p.validate().is_err());
}

struct Rig {
    seq: Sequencer,
    surface: Surface,
    events: Vec<Event>,
    fb: Vec<Feedback>,
}

impl Rig {
    fn new() -> Self {
        Rig { seq: Sequencer::new(Pattern::default(), 120_000), surface: Surface::default(), events: vec![], fb: vec![] }
    }
    fn press(&mut self, c: Control) -> bool {
        let (ev, fb) = (&mut self.events, &mut self.fb);
        self.surface.handle(c, &mut self.seq, 0, &mut |_, e| ev.push(e), &mut |f| fb.push(f)).0
    }
}

#[test]
fn encoder_turn_changes_pitch_of_that_step_on_selected_track() {
    let mut r = Rig::new();
    r.press(Control::TrackButton { index: 2, pressed: true });
    r.fb.clear();
    assert!(r.press(Control::EncoderTurn { index: 7, delta: 5 }));
    assert_eq!(r.seq.pattern.tracks[2].steps[7].note, 65);
    assert_eq!(r.seq.pattern.tracks[0].steps[7].note, 60);
    assert_eq!(r.fb, vec![Feedback::Ring { index: 7, value: 65 }]);
}

#[test]
fn pitch_clamps_at_midi_limits() {
    let mut r = Rig::new();
    for _ in 0..10 {
        let _ = r.press(Control::EncoderTurn { index: 0, delta: -63 });
    }
    assert_eq!(r.seq.pattern.tracks[0].steps[0].note, 0);
}

#[test]
fn encoders_do_nothing_when_another_parameter_is_selected() {
    let mut r = Rig::new();
    r.press(Control::ParamButton { index: 1, pressed: true }); // VELOCITY: not in milestone 1
    r.fb.clear();
    assert!(!r.press(Control::EncoderTurn { index: 0, delta: 5 }));
    assert_eq!(r.seq.pattern.tracks[0].steps[0].note, 60);
    assert_eq!(r.fb, vec![Feedback::Ring { index: 0, value: 0 }]); // true value echoed back
}

#[test]
fn encoder_push_toggles_step_and_release_does_nothing() {
    let mut r = Rig::new();
    assert!(r.press(Control::EncoderPush { index: 3, pressed: true }));
    assert!(!r.seq.pattern.tracks[0].steps[3].on);
    assert!(!r.press(Control::EncoderPush { index: 3, pressed: false }));
    assert!(!r.seq.pattern.tracks[0].steps[3].on);
    assert!(r.press(Control::EncoderPush { index: 3, pressed: true }));
    assert!(r.seq.pattern.tracks[0].steps[3].on);
}

#[test]
fn start_stop_button_and_footswitch_1_toggle_transport() {
    let mut r = Rig::new();
    assert!(!r.press(Control::Transport { which: Transport::StartStop, pressed: true }));
    assert!(r.seq.is_running());
    assert_eq!(r.events, vec![Event::Start]);
    r.press(Control::Footswitch { index: 0, pressed: true });
    assert!(!r.seq.is_running());
    assert_eq!(r.events.last(), Some(&Event::Stop));
    assert!(r.fb.contains(&Feedback::TransportLed { which: Transport::StartStop, on: false }));
}

#[test]
fn track_select_refreshes_rings_and_lights_for_that_track() {
    let mut r = Rig::new();
    r.seq.pattern.set_note(1, 9, 48);
    r.press(Control::TrackButton { index: 1, pressed: true });
    assert!(r.fb.contains(&Feedback::Ring { index: 9, value: 48 }));
    assert!(r.fb.contains(&Feedback::EncoderLed { index: 9, on: false })); // track 2 steps start off
    assert!(r.fb.contains(&Feedback::TrackLed { index: 1, on: true }));
    assert!(r.fb.contains(&Feedback::TrackLed { index: 0, on: false }));
    assert_eq!(r.fb.iter().filter(|f| matches!(f, Feedback::Ring { .. })).count(), 32);
}

#[test]
fn out_of_range_indices_are_ignored_not_panics() {
    let mut r = Rig::new();
    assert!(!r.press(Control::EncoderTurn { index: 40, delta: 1 }));
    assert!(!r.press(Control::EncoderPush { index: 40, pressed: true }));
    r.press(Control::TrackButton { index: 9, pressed: true });
    assert_eq!(r.surface.track, 0);
}
