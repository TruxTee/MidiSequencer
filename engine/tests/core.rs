use seq_engine::pattern::Pattern;
use seq_engine::sequencer::{Event, Sequencer};
use seq_engine::state::{load, save_atomic, Autosave, LoadOutcome, State};

const MS: u64 = 1_000_000;

/// Unique scratch directory per test, removed on drop.
struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!("seq-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(seq: &mut Sequencer, until_ns: u64) -> Vec<(u64, Event)> {
    let mut v = Vec::new();
    // advance() handles at most 1024 ticks per call; loop like the real caller does.
    for _ in 0..1_000_000 {
        let before = v.len();
        seq.advance(until_ns, &mut |t, e| v.push((t, e)));
        if v.len() == before {
            break;
        }
    }
    v
}

fn started(pattern: Pattern, bpm_milli: u32) -> Sequencer {
    let mut s = Sequencer::new(pattern, bpm_milli);
    s.start(0, &mut |_, _| {});
    s
}

fn note_ons(ev: &[(u64, Event)]) -> Vec<(u64, u8, u8)> {
    ev.iter()
        .filter_map(|&(t, e)| match e {
            Event::NoteOn { track, note, .. } => Some((t, track, note)),
            _ => None,
        })
        .collect()
}

#[test]
fn sixteenths_at_120_bpm_are_125_ms_apart_with_half_step_gate() {
    let mut s = started(Pattern::default(), 120_000);
    let ev = run(&mut s, 300 * MS);
    assert_eq!(note_ons(&ev), vec![(0, 0, 60), (125 * MS, 0, 60), (250 * MS, 0, 60)]);
    let first_off = ev.iter().find(|(_, e)| matches!(e, Event::NoteOff { .. })).unwrap();
    assert_eq!(first_off.0, 62_500_000);
}

#[test]
fn twenty_four_clocks_per_beat() {
    let mut s = started(Pattern::default(), 120_000);
    let ev = run(&mut s, 1000 * MS);
    let clocks = ev.iter().filter(|(_, e)| *e == Event::Clock).count();
    assert_eq!(clocks, 49); // ticks 0..=48 inclusive: 2 beats at 120 BPM
}

#[test]
fn pattern_wraps_after_32_steps() {
    let mut p = Pattern::default();
    p.set_note(0, 0, 64);
    p.set_note(0, 31, 70);
    let mut s = started(p, 120_000);
    let ons = note_ons(&run(&mut s, 4000 * MS));
    assert_eq!(ons[31], (3875 * MS, 0, 70));
    assert_eq!(ons[32], (4000 * MS, 0, 64));
}

#[test]
fn edit_while_sounding_releases_the_note_that_was_started() {
    let mut s = started(Pattern::default(), 120_000);
    run(&mut s, 0);
    s.pattern.set_note(0, 0, 72);
    let ev = run(&mut s, 62_500_000);
    assert!(ev.contains(&(62_500_000, Event::NoteOff { track: 0, note: 60 })));
}

#[test]
fn tempo_change_applies_from_next_tick() {
    let mut s = started(Pattern::default(), 120_000);
    run(&mut s, 0);
    s.set_tempo(60_000);
    // tick 1 keeps its 120 BPM time (20.833 ms), ticks 2..6 run at 60 BPM (41.667 ms each)
    let ons = note_ons(&run(&mut s, 300 * MS));
    assert_eq!(ons[0], (20_833_333 + 208_333_333, 0, 60)); // step 1; step 0 played before the change
}

#[test]
fn no_drift_after_one_hour() {
    let mut s = started(Pattern::default(), 120_000);
    let ev = run(&mut s, 3600 * 1000 * MS);
    assert_eq!(note_ons(&ev).last().unwrap().0, 3_600_000 * MS);
}

#[test]
fn stop_releases_sounding_notes_then_goes_silent() {
    let mut s = started(Pattern::default(), 120_000);
    run(&mut s, 10 * MS);
    let mut ev = Vec::new();
    s.stop(10 * MS, &mut |t, e| ev.push((t, e)));
    assert_eq!(ev, vec![(10 * MS, Event::NoteOff { track: 0, note: 60 }), (10 * MS, Event::Stop)]);
    assert!(run(&mut s, 5000 * MS).is_empty());
    assert_eq!(s.next_due(), None);
}

#[test]
fn tempo_is_clamped_so_zero_cannot_divide() {
    let mut s = Sequencer::new(Pattern::default(), 0);
    assert_eq!(s.bpm_milli(), 20_000);
    s.set_tempo(1_000_000);
    assert_eq!(s.bpm_milli(), 300_000);
}

#[test]
fn set_note_clamps_and_ignores_bad_indices() {
    let mut p = Pattern::default();
    p.set_note(1, 3, 200);
    p.set_note(1, 4, -5);
    let before = p.clone();
    p.set_note(9, 0, 50);
    p.set_note(0, 99, 50);
    assert_eq!(p, before);
    assert_eq!(p.tracks[1].steps[3].note, 127);
    assert_eq!(p.tracks[1].steps[4].note, 0);
}

#[test]
fn one_advance_call_is_bounded() {
    let mut s = started(Pattern::default(), 300_000);
    let mut clocks = 0;
    s.advance(u64::MAX / 2, &mut |_, e| clocks += (e == Event::Clock) as u32);
    assert_eq!(clocks, 1024);
}

#[test]
fn save_and_load_round_trip() {
    let dir = TempDir::new();
    let path = dir.path().join("state.json");
    let mut st = State::default();
    st.pattern.set_note(2, 5, 77);
    st.bpm_milli = 98_500;
    save_atomic(&path, &st).unwrap();
    let (back, outcome) = load(&path);
    assert_eq!(outcome, LoadOutcome::Loaded);
    assert_eq!(back.pattern.tracks[2].steps[5].note, 77);
    assert_eq!(back.bpm_milli, 98_500);
    assert!(!path.with_extension("tmp").exists());
}

#[test]
fn missing_file_gives_default() {
    let dir = TempDir::new();
    let (st, outcome) = load(&dir.path().join("none.json"));
    assert_eq!(outcome, LoadOutcome::Missing);
    assert_eq!(st, State::default());
}

#[test]
fn corrupt_file_is_moved_aside() {
    let dir = TempDir::new();
    let path = dir.path().join("state.json");
    std::fs::write(&path, b"{ half a fi").unwrap();
    let (st, outcome) = load(&path);
    assert_eq!(outcome, LoadOutcome::Corrupt(path.with_extension("bad")));
    assert_eq!(st, State::default());
    assert!(path.with_extension("bad").exists());
}

#[test]
fn cut_before_rename_keeps_old_state() {
    let dir = TempDir::new();
    let path = dir.path().join("state.json");
    let mut old = State::default();
    old.bpm_milli = 133_000;
    save_atomic(&path, &old).unwrap();
    std::fs::write(path.with_extension("tmp"), b"{ interrupted").unwrap();
    assert_eq!(load(&path).0.bpm_milli, 133_000);
}

#[test]
fn autosave_waits_for_quiet_but_not_forever() {
    let mut a = Autosave::new(3000 * MS, 10_000 * MS);
    assert!(!a.due(0));
    a.mark_dirty(0);
    assert!(!a.due(2999 * MS));
    assert!(a.due(3000 * MS));
    a.saved();
    assert!(!a.due(9000 * MS));

    let mut b = Autosave::new(3000 * MS, 10_000 * MS);
    for sec in 0..10 {
        b.mark_dirty(sec * 1000 * MS);
        assert!(!b.due(sec * 1000 * MS + 999 * MS));
    }
    assert!(b.due(10_000 * MS));
}
