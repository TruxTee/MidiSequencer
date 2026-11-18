use seq_engine::control::Profile;
use seq_engine::midi::Routing;
use seq_engine::ports::{resolve, wiring};

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn profile() -> Profile {
    serde_json::from_slice(&std::fs::read("../profiles/pcr32.json").unwrap()).unwrap()
}

fn routing() -> Routing {
    serde_json::from_slice(&std::fs::read("routing.json").unwrap()).unwrap()
}

#[test]
fn resolve_prefers_exact_name_over_substring() {
    let n = names(&["PCR32 in", "PCR32"]);
    assert_eq!(resolve(&n, "pcr32").map(String::as_str), Some("PCR32"));
    let n = names(&["rtpmidid:PCR32 128:2", "Midi Through"]);
    assert_eq!(resolve(&n, "PCR32").map(String::as_str), Some("rtpmidid:PCR32 128:2"));
    assert_eq!(resolve(&n, "Laptop"), None);
}

#[test]
fn wiring_follows_ports_going_and_coming_back() {
    let (p, r) = (profile(), routing());
    let ins = names(&["rtpmidid:PCR32 128:2"]);
    let outs = names(&["rtpmidid:Laptop 128:0", "rtpmidid:PCR32 128:2"]);
    let up = wiring(&ins, &outs, &p, &r);
    assert_eq!(up.controller_in.as_deref(), Some("rtpmidid:PCR32 128:2"));
    assert_eq!(up.feedback.as_deref(), Some("rtpmidid:PCR32 128:2"));
    assert!(up.outputs.iter().all(|o| o.as_deref() == Some("rtpmidid:Laptop 128:0")));

    // Phone drops: controller gone, instruments untouched.
    let down = wiring(&[], &names(&["rtpmidid:Laptop 128:0"]), &p, &r);
    assert_eq!(down.controller_in, None);
    assert_eq!(down.feedback, None);
    assert_eq!(down.outputs, up.outputs);

    // Phone back under the same name: same wiring as before, which differs from `down`.
    let back = wiring(&ins, &outs, &p, &r);
    assert_eq!(back, up);
    assert_ne!(back, down);
}
