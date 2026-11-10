// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::scripting::value::{HandleKind, MapKey, Value};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    net::SocketAddr,
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Field {
    Flag(bool),
    Number(f64),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptPacket {
    pub topic: String,
    fields: BTreeMap<String, Field>,
}

impl ScriptPacket {
    pub fn valid(&self) -> bool {
        let identifier = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        };
        identifier(&self.topic)
            && self.fields.len() <= 64
            && self.fields.iter().all(|(key, value)| {
                identifier(key)
                    && match value {
                        Field::Text(s) => s.len() <= 512,
                        Field::Number(n) => n.is_finite(),
                        Field::Flag(_) => true,
                    }
            })
            && serde_json::to_vec(self).is_ok_and(|v| v.len() <= 4096)
    }

    fn from_args(args: &[Value]) -> Result<Self, String> {
        let [topic, fields] = args else {
            return Err("expected a topic and a map".into());
        };
        let map = fields.as_map()?;
        let mut packet = Self {
            topic: topic.as_string()?.into(),
            fields: BTreeMap::new(),
        };
        for (key, value) in map.borrow().iter() {
            let MapKey::String(key) = key else {
                return Err("network fields require string keys".into());
            };
            let field = match value {
                Value::String(s) => Field::Text(s.clone()),
                Value::Number(n) => Field::Number(*n),
                Value::Bool(b) => Field::Flag(*b),
                _ => return Err("network fields require text, numbers or booleans".into()),
            };
            packet.fields.insert(key.clone(), field);
        }
        if !packet.valid() {
            return Err("invalid network topic or payload limit exceeded".into());
        }
        Ok(packet)
    }

    pub fn arguments(self) -> Vec<Value> {
        let fields = self
            .fields
            .into_iter()
            .map(|(key, field)| {
                (
                    MapKey::String(key),
                    match field {
                        Field::Text(s) => Value::String(s),
                        Field::Number(n) => Value::Number(n),
                        Field::Flag(b) => Value::Bool(b),
                    },
                )
            })
            .collect();
        vec![Value::String(self.topic), Value::map(fields)]
    }
}

#[derive(Clone, Debug)]
pub enum InviteRequest {
    Host { max_players: usize },
    Join { link: String },
}

#[derive(Debug, Default)]
pub struct SessionIo {
    pub invite_request: Option<InviteRequest>,
    pub invite_link: String,
    pub leave_requested: bool,
    pub start: Option<(NetworkRole, SocketAddr)>,
    pub address: String,
    pub address_input: Option<String>,
    pub error: String,
    pub players: BTreeMap<u64, Option<u64>>,
    pub requests: VecDeque<ScriptPacket>,
    pub states: BTreeMap<String, (u64, ScriptPacket)>,
    revision: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetworkRole {
    #[default]
    Solo,
    Host,
    Client,
}

impl NetworkRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solo => "solo",
            Self::Host => "host",
            Self::Client => "client",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NetworkScriptContext {
    pub role: NetworkRole,
    pub player_id: Option<u64>,
    pub connected: bool,
    pub io: Option<Rc<RefCell<SessionIo>>>,
}

impl NetworkScriptContext {
    pub fn host() -> Self {
        Self {
            role: NetworkRole::Host,
            player_id: Some(1),
            connected: true,
            io: None,
        }
    }

    pub fn client(player_id: Option<u64>, connected: bool) -> Self {
        Self {
            role: NetworkRole::Client,
            player_id,
            connected,
            io: None,
        }
    }

    pub fn is_server(&self) -> bool {
        matches!(self.role, NetworkRole::Solo | NetworkRole::Host)
    }

    pub fn is_local_player(&self, player_id: u64) -> bool {
        self.player_id == Some(player_id)
    }

    pub fn call(&self, method: &str, args: &[Value]) -> Result<Value, String> {
        let io = self
            .io
            .as_ref()
            .ok_or("network sessions require the standalone runtime")?;
        let mut io = io.borrow_mut();
        match method {
            "host_invite" if self.role == NetworkRole::Solo && !self.connected => {
                let max_players = if args.is_empty() { 2 } else if args.len() == 1 {
                    args[0].as_index()?.clamp(2, 8)
                } else { return Err("network.host_invite accepts an optional player count".into()); };
                if io.invite_request.is_some() { return Ok(Value::Bool(false)); }
                io.invite_request = Some(InviteRequest::Host { max_players });
                io.error.clear();
            }
            "join_invite" if self.role == NetworkRole::Solo && !self.connected => {
                let [value] = args else { return Err("network.join_invite expects a link or invite code".into()); };
                if io.invite_request.is_some() { return Ok(Value::Bool(false)); }
                io.invite_request = Some(InviteRequest::Join { link: value.as_string()?.to_owned() });
                io.error.clear();
            }
            "invite_link" if args.is_empty() => return Ok(Value::String(io.invite_link.clone())),
            "leave" if args.is_empty() => {
                io.leave_requested = true;
            }
            "join" if args.is_empty() => return Err("network.join requires a host address; for invitations use network.join_invite(link)".into()),
            "host" | "join" => {
                let [value] = args else { return Err("expected an IP address and port".into()); };
                if self.connected || io.start.is_some() { return Ok(Value::Bool(false)); }
                let Ok(addr) = value.as_string()?.parse::<SocketAddr>() else {
                    io.error = "Enter an IP address and port".into();
                    return Ok(Value::Bool(false));
                };
                io.error.clear();
                io.start = Some((if method == "host" {NetworkRole::Host} else {NetworkRole::Client}, addr));
            }
            "players" if args.is_empty() => return Ok(Value::array(io.players.keys().map(|id| Value::Number(*id as f64)).collect())),
            "address" if args.is_empty() => return Ok(Value::String(io.address.clone())),
            "error" if args.is_empty() => return Ok(Value::String(io.error.clone())),
            "character" if args.len() == 1 => {
                let id = args[0].as_index()? as u64;
                return Ok(io.players.get(&id).copied().flatten().map_or(Value::Nil, |id| Value::Handle {kind: HandleKind::Entity, id}));
            }
            "request" if self.role == NetworkRole::Client && self.connected => {
                if io.requests.len() >= 16 { return Ok(Value::Bool(false)); }
                io.requests.push_back(ScriptPacket::from_args(args)?);
            }
            "publish" if self.role == NetworkRole::Host => {
                let packet = ScriptPacket::from_args(args)?;
                if !io.states.contains_key(&packet.topic) && io.states.len() >= 32 { return Ok(Value::Bool(false)); }
                io.revision += 1;
                let revision = io.revision;
                io.states.insert(packet.topic.clone(), (revision, packet));
            }
            _ => return Err(format!("network.{method} unavailable for role {} or invalid arguments", self.role.as_str())),
        }
        Ok(Value::Bool(true))
    }

    pub fn authorize_shared_write(
        &self,
        operation: &str,
        target: Option<(HandleKind, u64)>,
    ) -> Result<(), String> {
        if self.is_server() {
            return Ok(());
        }
        let entity = target
            .map(|(kind, id)| format!(" {}#{}", kind.name(), id))
            .unwrap_or_default();
        Err(format!(
            "AeoScript network authority denied: operation '{}{}' requires host/server authority; role={}, player_id={}. Send a validated client request instead.",
            operation,
            entity,
            self.role.as_str(),
            self.player_id
                .map_or_else(|| "unassigned".to_owned(), |id| id.to_string()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_scripts_keep_unrestricted_legacy_writes() {
        let context = NetworkScriptContext::default();
        assert_eq!(context.role, NetworkRole::Solo);
        assert!(context.is_server());
        assert!(context.authorize_shared_write("cell.create", None).is_ok());
        assert!(context.authorize_shared_write("save.commit", None).is_ok());
    }

    #[test]
    fn only_hosts_can_change_shared_state() {
        let host = NetworkScriptContext::host();
        let client = NetworkScriptContext::client(Some(2), true);
        assert!(host.authorize_shared_write("spawn", None).is_ok());
        let error = client
            .authorize_shared_write("set_property", Some((HandleKind::Actor, 33)))
            .unwrap_err();
        assert!(error.contains("set_property Actor#33"));
        assert!(error.contains("role=client"));
        assert!(error.contains("player_id=2"));
    }

    #[test]
    fn unassigned_clients_cannot_write_or_impersonate_other_players() {
        let pending = NetworkScriptContext::client(None, false);
        assert!(!pending.is_server());
        assert!(!pending.is_local_player(1));
        assert!(
            pending
                .authorize_shared_write("save.set", None)
                .unwrap_err()
                .contains("unassigned")
        );
        let assigned = NetworkScriptContext::client(Some(5), true);
        assert!(assigned.is_local_player(5));
        assert!(!assigned.is_local_player(4));
    }
}
