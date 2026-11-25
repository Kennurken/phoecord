//! Gateway bridge: what a registry handler detour hands to `Engine::on_dispatch`.
//!
//! The registry gives the already-decoded event, so there is no wire format here: the event name
//! and its `d` as JSON bytes go in, a verdict comes out. Whether the detour then calls the original
//! handler (`Pass`, `Replace`) or skips it (`Drop`) is the detour's job.

use phoecord_core::Val;
use phoecord_transport::Engine;

#[derive(Debug, PartialEq)]
pub enum GatewayAction {
    /// Call the original handler with the original data.
    Pass,
    /// Call it with this `d` instead.
    Replace(Vec<u8>),
    /// Do not call it.
    Drop,
}

#[derive(Debug)]
pub struct GatewayOutcome {
    pub action: GatewayAction,
    /// Extra server → client payloads plugins queued; delivery belongs to the transport (2.2.7).
    pub extra: Vec<Val>,
    /// Client → server payloads plugins queued (`Host::emit_send`).
    pub sends: Vec<(i64, Val)>,
}

/// One dispatch event: `name` (`MESSAGE_CREATE`) and its `d` as JSON.
pub fn dispatch(engine: &Engine, name: &str, data: &[u8]) -> Result<GatewayOutcome, String> {
    if !engine.wants_gateway() {
        return Ok(GatewayOutcome {
            action: GatewayAction::Pass,
            extra: Vec::new(),
            sends: Vec::new(),
        });
    }
    let d: serde_json::Value =
        serde_json::from_slice(data).map_err(|e| format!("{name}: data is not JSON ({e})"))?;
    let mut payload = Val::map();
    payload.set("op", Val::Int(0));
    payload.set("t", Val::Str(name.to_string()));
    payload.set("d", Val::from_json(d));
    let r = engine.on_dispatch(payload);
    let action = if r.drop {
        GatewayAction::Drop
    } else if r.changed {
        let d = r.payload.get("d").unwrap_or(&Val::Null);
        let bytes = serde_json::to_vec(&d.to_json())
            .map_err(|e| format!("{name}: cannot encode data: {e}"))?;
        GatewayAction::Replace(bytes)
    } else {
        GatewayAction::Pass
    };
    Ok(GatewayOutcome {
        action,
        extra: r.extra,
        sends: r.sends,
    })
}
