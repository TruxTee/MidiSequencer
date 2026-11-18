//! Which existing ports the engine should be connected to. Pure: works on port names, so the
//! watcher thread can re-check often and the main loop only reconnects what changed.
//! A controller or instrument that drops off and comes back is picked up again this way.

use crate::control::Profile;
use crate::midi::Routing;
use crate::pattern::TRACKS;

/// Exact name first (case-insensitive), then the first name containing `want`.
pub fn resolve<'a>(names: &'a [String], want: &str) -> Option<&'a String> {
    let want = want.to_lowercase();
    names
        .iter()
        .find(|n| n.to_lowercase() == want)
        .or_else(|| names.iter().find(|n| n.to_lowercase().contains(&want)))
}

/// The ports the engine wants right now; None where nothing matches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wiring {
    pub controller_in: Option<String>,
    pub feedback: Option<String>,
    /// One entry per track.
    pub outputs: [Option<String>; TRACKS],
}

/// `inputs` are ports the engine can read from, `outputs` ports it can write to.
pub fn wiring(inputs: &[String], outputs: &[String], profile: &Profile, routing: &Routing) -> Wiring {
    let mut w = Wiring {
        controller_in: resolve(inputs, &profile.port).cloned(),
        feedback: resolve(outputs, profile.feedback_port.as_deref().unwrap_or(&profile.port)).cloned(),
        ..Wiring::default()
    };
    for (t, inst) in routing.instruments.iter().take(TRACKS).enumerate() {
        w.outputs[t] = resolve(outputs, &inst.port).cloned();
    }
    w
}
