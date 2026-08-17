use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::manifest::Manifest;

/// The wire protocol version announced in [`Msg::Hello`] as `v`. The host
/// rejects a handshake that carries any other value.
pub const API_VERSION: u32 = 1;

/// Name of the canonical test-plugin fixture: what it announces in
/// [`Msg::Hello`] and what the host sends as `cmd` on `invoke`/`view.interact`.
pub const PLUGIN_NAME: &str = "hello";

/// Custom id of the fixture's click button: rendered in the view and echoed
/// back in `view.interact` args to trigger the click path.
pub const BUTTON_CUSTOM_ID: &str = "hello:click";

/// A message on the plugin wire, serialized as one compact JSON object per
/// line. The `t` discriminator names the variant: `hello`, `call`, `resp`,
/// `event`, `ping`, `pong`, `bye`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Msg {
    /// Startup handshake: the plugin's first line after spawn.
    Hello {
        /// Wire protocol version.
        v: u32,
        /// Plugin name.
        name: String,
        /// Capabilities the plugin serves and host ops it requires.
        caps: Vec<String>,
        /// The plugin's manifest declaration, when the plugin carries one.
        /// Absent on old hellos, which the host accepts (validated only when
        /// present).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manifest: Option<Manifest>,
    },
    /// A request that expects a [`Msg::Resp`] with the same `id`.
    Call {
        /// Monotonic per-producer correlation id.
        id: u64,
        /// Operation name, e.g. `invoke`, `view.interact`, `host.fetch_user`.
        op: String,
        /// Command name for `invoke`; absent for host-service ops.
        #[serde(skip_serializing_if = "Option::is_none")]
        cmd: Option<String>,
        /// Opaque arguments, passed through verbatim.
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<Value>,
    },
    /// The reply to a [`Msg::Call`], echoing the caller's `id`.
    Resp {
        /// Correlation id of the [`Msg::Call`] being answered.
        id: u64,
        /// `true` on success (with `data`), `false` on failure (with `error`).
        ok: bool,
        /// Successful result payload.
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
        /// First-class wire error.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<WireError>,
    },
    /// One-way push; never answered.
    Event {
        /// Event name, e.g. `view.timeout`.
        name: String,
        /// Opaque event payload.
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
    },
    /// Liveness probe, host to plugin.
    Ping,
    /// Liveness reply, plugin to host.
    Pong,
    /// Graceful shutdown, host to plugin.
    Bye,
}

impl Msg {
    /// Builds a successful [`Msg::Resp`] (`ok: true`, `error` always absent).
    /// Pass `None` as `data` for a success with no payload.
    pub fn resp_ok(id: u64, data: Option<Value>) -> Msg {
        Msg::Resp {
            id,
            ok: true,
            data,
            error: None,
        }
    }

    /// Builds a failed [`Msg::Resp`] (`ok: false`, `data` always absent).
    pub fn resp_err(id: u64, error: WireError) -> Msg {
        Msg::Resp {
            id,
            ok: false,
            data: None,
            error: Some(error),
        }
    }
}

/// A first-class error carried on the wire in a failed [`Msg::Resp`]. Panics
/// never cross the wire; failures always take this shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    /// Machine-readable error kind, e.g. `UnknownAction`.
    pub kind: String,
    /// Human-readable error message.
    pub msg: String,
}

/// Monotonic per-producer call-id source. Each side of the wire owns its own
/// sequence; ids are only meaningful within the producer that issued them and
/// are never reused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallIdSeq {
    next: u64,
}

impl CallIdSeq {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the next id, increasing by one per call. Panics if the
    /// sequence is exhausted (after 2^64 ids); ids are never reused.
    pub fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .expect("call-id sequence exhausted");
        id
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn round_trip(msg: &Msg) -> Msg {
        let json = serde_json::to_string(msg).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    // ── wire format ──────────────────────────────────────────────────────────

    #[test]
    fn hello_matches_wire_format() {
        let msg = Msg::Hello {
            v: API_VERSION,
            name: "feed".into(),
            caps: vec!["command:feed".into()],
            manifest: None,
        };
        assert_eq!(
            serde_json::to_string(&msg).unwrap(),
            r#"{"t":"hello","v":1,"name":"feed","caps":["command:feed"]}"#
        );
    }

    #[test]
    fn hello_with_manifest_matches_wire_format() {
        let msg = Msg::Hello {
            v: API_VERSION,
            name: "feed".into(),
            caps: vec!["command:feed".into()],
            manifest: Some(Manifest {
                name: "feed".into(),
                description: "Feed subscriptions".into(),
                version: "0.1.0".into(),
                commands: vec![crate::manifest::CommandDef {
                    create_command: json!({"name": "feed.list", "description": "List feeds"}),
                }],
                event_handlers: vec![],
                tasks: vec![],
                api_version: API_VERSION,
            }),
        };
        assert_eq!(
            serde_json::to_string(&msg).unwrap(),
            r#"{"t":"hello","v":1,"name":"feed","caps":["command:feed"],"manifest":{"name":"feed","description":"Feed subscriptions","version":"0.1.0","commands":[{"create_command":{"description":"List feeds","name":"feed.list"}}],"event_handlers":[],"tasks":[],"api_version":1}}"#
        );
    }

    #[test]
    fn call_host_to_plugin_matches_wire_format() {
        let msg = Msg::Call {
            id: 7,
            op: "invoke".into(),
            cmd: Some("feed.list".into()),
            args: Some(json!({"guild_id": "123"})),
        };
        assert_eq!(
            serde_json::to_string(&msg).unwrap(),
            r#"{"t":"call","id":7,"op":"invoke","cmd":"feed.list","args":{"guild_id":"123"}}"#
        );
    }

    #[test]
    fn call_plugin_to_host_omits_missing_fields() {
        let msg = Msg::Call {
            id: 3,
            op: "host.fetch_user".into(),
            cmd: None,
            args: None,
        };
        assert_eq!(
            serde_json::to_string(&msg).unwrap(),
            r#"{"t":"call","id":3,"op":"host.fetch_user"}"#
        );
    }

    #[test]
    fn resp_ok_matches_wire_format() {
        let msg = Msg::resp_ok(7, Some(json!({"items": []})));
        assert_eq!(
            serde_json::to_string(&msg).unwrap(),
            r#"{"t":"resp","id":7,"ok":true,"data":{"items":[]}}"#
        );
    }

    #[test]
    fn resp_error_is_first_class_wire_value() {
        let msg = Msg::resp_err(
            7,
            WireError {
                kind: "UnknownAction".into(),
                msg: "no such id".into(),
            },
        );
        assert_eq!(
            serde_json::to_string(&msg).unwrap(),
            r#"{"t":"resp","id":7,"ok":false,"error":{"kind":"UnknownAction","msg":"no such id"}}"#
        );
    }

    #[test]
    fn event_matches_wire_format() {
        let msg = Msg::Event {
            name: "guild.ready".into(),
            data: Some(json!({"guild_id": "123"})),
        };
        assert_eq!(
            serde_json::to_string(&msg).unwrap(),
            r#"{"t":"event","name":"guild.ready","data":{"guild_id":"123"}}"#
        );
    }

    #[test]
    fn ping_pong_bye_match_wire_format() {
        assert_eq!(
            serde_json::to_string(&Msg::Ping).unwrap(),
            r#"{"t":"ping"}"#
        );
        assert_eq!(
            serde_json::to_string(&Msg::Pong).unwrap(),
            r#"{"t":"pong"}"#
        );
        assert_eq!(serde_json::to_string(&Msg::Bye).unwrap(), r#"{"t":"bye"}"#);
    }

    // ── round trips ──────────────────────────────────────────────────────────

    #[test]
    fn every_variant_round_trips_losslessly() {
        let msgs = [
            Msg::Hello {
                v: API_VERSION,
                name: "feed".into(),
                caps: vec!["command:feed".into()],
                manifest: None,
            },
            Msg::Call {
                id: 1,
                op: "invoke".into(),
                cmd: Some("feed.list".into()),
                args: None,
            },
            Msg::Call {
                id: 2,
                op: "host.fetch_user".into(),
                cmd: None,
                args: None,
            },
            Msg::Resp {
                id: 1,
                ok: true,
                data: Some(json!({"items": []})),
                error: None,
            },
            Msg::resp_ok(3, None),
            Msg::Resp {
                id: 2,
                ok: false,
                data: None,
                error: Some(WireError {
                    kind: "UnknownOp".into(),
                    msg: "no such op".into(),
                }),
            },
            Msg::Event {
                name: "view.timeout".into(),
                data: None,
            },
            Msg::Ping,
            Msg::Pong,
            Msg::Bye,
        ];
        for msg in msgs {
            assert_eq!(round_trip(&msg), msg);
        }
    }

    #[test]
    fn wire_decodes_unknown_fields_gracefully() {
        let json = r#"{"t":"hello","v":1,"name":"feed","caps":[],"ver":"0.1.0"}"#;
        let msg: Msg = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, Msg::Hello { .. }));
    }

    // ── correlation ids ──────────────────────────────────────────────────────

    #[test]
    fn call_id_seq_is_monotonic_per_producer() {
        let mut host = CallIdSeq::new();
        let mut plugin = CallIdSeq::new();

        assert_eq!(host.next_id(), 0);
        assert_eq!(host.next_id(), 1);
        assert_eq!(plugin.next_id(), 0);
        assert_eq!(host.next_id(), 2);
        assert_eq!(plugin.next_id(), 1);
    }
}
