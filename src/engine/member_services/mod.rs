// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.

mod client;
mod publish;
pub(crate) use client::{MemberClient, MemberDisplay, PublishStatus};

use serde::Deserialize;
use std::fmt;
use std::io::Read;
use std::time::{Duration, Instant};

const MEMBER_ORIGIN: &str = "https://aeowun.com";
const CLIENT_ID: &str = "worldkiln-editor";
const AUTHORIZE_PATH: &str = "/api/members/auth/device/authorize";
const TOKEN_PATH: &str = "/api/members/auth/device/token";
const SESSION_PATH: &str = "/api/members/auth/device/session";
const REVOKE_PATH: &str = "/api/members/auth/device/revoke";
const MAX_BODY_BYTES: usize = 16 * 1024;
const MIN_POLL_SECONDS: u64 = 5;
const MAX_POLL_SECONDS: u64 = 60;
const MAX_GRANT_SECONDS: u64 = 1800;

#[derive(PartialEq, Eq)]
pub(crate) enum AuthError {
    AlreadyInProgress,
    NoPendingAuthorization,
    InvalidResponse,
    ServiceUnavailable,
    IdentityUnverified,
    RevocationFailed,
}

impl fmt::Debug for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AlreadyInProgress => "AlreadyInProgress",
            Self::NoPendingAuthorization => "NoPendingAuthorization",
            Self::InvalidResponse => "InvalidResponse",
            Self::ServiceUnavailable => "ServiceUnavailable",
            Self::IdentityUnverified => "IdentityUnverified",
            Self::RevocationFailed => "RevocationFailed",
        })
    }
}

struct Secret(String);

impl Secret {
    fn new(value: String) -> Result<Self, AuthError> {
        if value.len() < 16
            || value.len() > 512
            || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'~'))
        {
            return Err(AuthError::InvalidResponse);
        }
        Ok(Self(value))
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DevicePrompt {
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemberIdentity {
    pub id: String,
    pub handle: String,
    pub display_name: String,
}

struct DeviceGrant {
    device_code: Secret,
    prompt: DevicePrompt,
    expires_in: Duration,
    interval: Duration,
}

enum TokenResult {
    Pending,
    SlowDown,
    Denied,
    Expired,
    Authorized {
        access_token: Secret,
        expires_in: Duration,
    },
}

trait DeviceTransport {
    fn begin(&self) -> Result<DeviceGrant, AuthError>;
    fn poll(&self, device_code: &str) -> Result<TokenResult, AuthError>;
    fn member(&self, access_token: &str) -> Result<MemberIdentity, AuthError>;
    fn revoke(&self, access_token: &str) -> Result<(), AuthError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LinkStatus {
    SignedOut,
    AwaitingApproval(DevicePrompt),
    SignedIn(MemberIdentity),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PollProgress {
    NotDue,
    Pending,
    SlowDown,
    Linked,
    Denied,
    Expired,
}

struct PendingGrant {
    code: Secret,
    prompt: DevicePrompt,
    expires_at: Instant,
    next_poll: Instant,
    interval: Duration,
}

struct LinkedMember {
    token: Secret,
    identity: MemberIdentity,
    expires_at: Instant,
}

enum LinkState {
    SignedOut,
    Pending(PendingGrant),
    Linked(LinkedMember),
}

pub(crate) struct MemberDeviceLink {
    state: LinkState,
}

impl Default for MemberDeviceLink {
    fn default() -> Self {
        Self {
            state: LinkState::SignedOut,
        }
    }
}

impl MemberDeviceLink {
    fn access_token(&self, now: Instant) -> Option<&str> {
        match &self.state {
            LinkState::Linked(linked) if now < linked.expires_at => Some(linked.token.expose()),
            _ => None,
        }
    }

    pub(crate) fn status(&self, now: Instant) -> LinkStatus {
        match &self.state {
            LinkState::SignedOut => LinkStatus::SignedOut,
            LinkState::Pending(pending) if now < pending.expires_at => {
                LinkStatus::AwaitingApproval(pending.prompt.clone())
            }
            LinkState::Linked(linked) if now < linked.expires_at => {
                LinkStatus::SignedIn(linked.identity.clone())
            }
            _ => LinkStatus::SignedOut,
        }
    }

    fn begin<T: DeviceTransport>(&mut self, transport: &T, now: Instant) -> Result<DevicePrompt, AuthError> {
        if !matches!(self.state, LinkState::SignedOut) {
            return Err(AuthError::AlreadyInProgress);
        }
        let grant = transport.begin()?;
        if grant.expires_in.is_zero() || grant.expires_in > Duration::from_secs(MAX_GRANT_SECONDS) {
            return Err(AuthError::InvalidResponse);
        }
        let interval = grant.interval.max(Duration::from_secs(MIN_POLL_SECONDS))
            .min(Duration::from_secs(MAX_POLL_SECONDS));
        validate_prompt(&grant.prompt)?;
        let prompt = grant.prompt.clone();
        self.state = LinkState::Pending(PendingGrant {
            code: grant.device_code,
            prompt: grant.prompt,
            expires_at: now + grant.expires_in,
            next_poll: now + interval,
            interval,
        });
        Ok(prompt)
    }

    fn poll<T: DeviceTransport>(&mut self, transport: &T, now: Instant) -> Result<PollProgress, AuthError> {
        let LinkState::Pending(grant) = &mut self.state else {
            return Err(AuthError::NoPendingAuthorization);
        };
        if now >= grant.expires_at {
            self.state = LinkState::SignedOut;
            return Ok(PollProgress::Expired);
        }
        if now < grant.next_poll {
            return Ok(PollProgress::NotDue);
        }
        grant.next_poll = now + grant.interval;
        match transport.poll(grant.code.expose())? {
            TokenResult::Pending => Ok(PollProgress::Pending),
            TokenResult::SlowDown => {
                if let LinkState::Pending(grant) = &mut self.state {
                    grant.interval = (grant.interval + Duration::from_secs(5))
                        .min(Duration::from_secs(MAX_POLL_SECONDS));
                    grant.next_poll = now + grant.interval;
                }
                Ok(PollProgress::SlowDown)
            }
            TokenResult::Denied => {
                self.state = LinkState::SignedOut;
                Ok(PollProgress::Denied)
            }
            TokenResult::Expired => {
                self.state = LinkState::SignedOut;
                Ok(PollProgress::Expired)
            }
            TokenResult::Authorized { access_token, expires_in } => {
                if expires_in.is_zero() || expires_in > Duration::from_secs(86400) {
                    return Err(AuthError::InvalidResponse);
                }
                let identity = match transport.member(access_token.expose()) {
                    Ok(identity) if valid_identity(&identity) => identity,
                    _ => {
                        let _ = transport.revoke(access_token.expose());
                        self.state = LinkState::SignedOut;
                        return Err(AuthError::IdentityUnverified);
                    }
                };
                self.state = LinkState::Linked(LinkedMember {
                    token: access_token,
                    identity,
                    expires_at: now + expires_in,
                });
                Ok(PollProgress::Linked)
            }
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.state = LinkState::SignedOut;
    }

    fn sign_out<T: DeviceTransport>(&mut self, transport: &T) -> Result<(), AuthError> {
        let previous = std::mem::replace(&mut self.state, LinkState::SignedOut);
        match previous {
            LinkState::Linked(linked) => transport
                .revoke(linked.token.expose())
                .map_err(|_| AuthError::RevocationFailed),
            _ => Ok(()),
        }
    }
}

fn valid_identity(member: &MemberIdentity) -> bool {
    !member.id.is_empty()
        && member.id.len() <= 128
        && !member.handle.is_empty()
        && member.handle.len() <= 64
        && !member.display_name.is_empty()
        && member.display_name.len() <= 128
        && [member.id.as_str(), member.handle.as_str(), member.display_name.as_str()]
            .iter()
            .all(|value| !value.chars().any(char::is_control))
}

fn trusted_verification_uri(value: &str) -> bool {
    let Ok(uri) = reqwest::Url::parse(value) else { return false; };
    uri.scheme() == "https"
        && uri.host_str() == Some("aeowun.com")
        && uri.port_or_known_default() == Some(443)
        && uri.username().is_empty()
        && uri.password().is_none()
}

fn validate_prompt(prompt: &DevicePrompt) -> Result<(), AuthError> {
    if prompt.user_code.is_empty()
        || prompt.user_code.len() > 32
        || !prompt.user_code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !trusted_verification_uri(&prompt.verification_uri)
        || prompt.verification_uri_complete.as_ref().is_some_and(|uri| !trusted_verification_uri(uri))
    {
        Err(AuthError::InvalidResponse)
    } else {
        Ok(())
    }
}

#[derive(Deserialize)]
struct AuthorizationPayload {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    #[serde(default = "default_poll_seconds")]
    interval: u64,
}

fn default_poll_seconds() -> u64 {
    MIN_POLL_SECONDS
}

fn parse_grant(payload: &[u8]) -> Result<DeviceGrant, AuthError> {
    if payload.len() > MAX_BODY_BYTES {
        return Err(AuthError::InvalidResponse);
    }
    let value: AuthorizationPayload =
        serde_json::from_slice(payload).map_err(|_| AuthError::InvalidResponse)?;
    let grant = DeviceGrant {
        device_code: Secret::new(value.device_code)?,
        prompt: DevicePrompt {
            user_code: value.user_code,
            verification_uri: value.verification_uri,
            verification_uri_complete: value.verification_uri_complete,
        },
        expires_in: Duration::from_secs(value.expires_in),
        interval: Duration::from_secs(value.interval),
    };
    validate_prompt(&grant.prompt)?;
    Ok(grant)
}

#[derive(Deserialize)]
struct TokenPayload {
    access_token: String,
    token_type: String,
    expires_in: u64,
}

fn parse_token(payload: &[u8]) -> Result<TokenResult, AuthError> {
    if payload.len() > MAX_BODY_BYTES {
        return Err(AuthError::InvalidResponse);
    }
    let value: TokenPayload =
        serde_json::from_slice(payload).map_err(|_| AuthError::InvalidResponse)?;
    if !value.token_type.eq_ignore_ascii_case("bearer") {
        return Err(AuthError::InvalidResponse);
    }
    Ok(TokenResult::Authorized {
        access_token: Secret::new(value.access_token)?,
        expires_in: Duration::from_secs(value.expires_in),
    })
}

#[derive(Deserialize)]
struct MemberPayload {
    authenticated: bool,
    member: Option<MemberIdentityPayload>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemberIdentityPayload {
    id: String,
    handle: String,
    display_name: String,
}

fn parse_member(payload: &[u8]) -> Result<MemberIdentity, AuthError> {
    if payload.len() > MAX_BODY_BYTES {
        return Err(AuthError::InvalidResponse);
    }
    let value: MemberPayload =
        serde_json::from_slice(payload).map_err(|_| AuthError::IdentityUnverified)?;
    let member = value.member.ok_or(AuthError::IdentityUnverified)?;
    let identity = MemberIdentity {
        id: member.id,
        handle: member.handle,
        display_name: member.display_name,
    };
    if !value.authenticated || !valid_identity(&identity) {
        return Err(AuthError::IdentityUnverified);
    }
    Ok(identity)
}

fn parse_error(payload: &[u8]) -> Result<TokenResult, AuthError> {
    #[derive(Deserialize)]
    struct ErrorPayload {
        error: String,
    }
    if payload.len() > MAX_BODY_BYTES {
        return Err(AuthError::InvalidResponse);
    }
    let error: ErrorPayload =
        serde_json::from_slice(payload).map_err(|_| AuthError::ServiceUnavailable)?;
    match error.error.as_str() {
        "authorization_pending" => Ok(TokenResult::Pending),
        "slow_down" => Ok(TokenResult::SlowDown),
        "access_denied" => Ok(TokenResult::Denied),
        "expired_token" => Ok(TokenResult::Expired),
        _ => Err(AuthError::ServiceUnavailable),
    }
}

pub(crate) struct HttpDeviceTransport {
    client: reqwest::blocking::Client,
}

impl HttpDeviceTransport {
    pub(crate) fn new() -> Result<Self, AuthError> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Worldkiln-MemberAuthorization")
            .build()
            .map_err(|_| AuthError::ServiceUnavailable)?;
        Ok(Self { client })
    }

    fn response_bytes(mut response: reqwest::blocking::Response) -> Result<Vec<u8>, AuthError> {
        if response.content_length().is_some_and(|length| length > MAX_BODY_BYTES as u64) {
            return Err(AuthError::InvalidResponse);
        }
        let mut body = Vec::new();
        response
            .take((MAX_BODY_BYTES + 1) as u64)
            .read_to_end(&mut body)
            .map_err(|_| AuthError::ServiceUnavailable)?;
        if body.len() > MAX_BODY_BYTES {
            return Err(AuthError::InvalidResponse);
        }
        Ok(body)
    }

    fn post_form(&self, path: &str, form: &str) -> Result<reqwest::blocking::Response, AuthError> {
        self.client
            .post(format!("{MEMBER_ORIGIN}{path}"))
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CACHE_CONTROL, "no-store")
            .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(form.to_owned())
            .send()
            .map_err(|_| AuthError::ServiceUnavailable)
    }
}

impl DeviceTransport for HttpDeviceTransport {
    fn begin(&self) -> Result<DeviceGrant, AuthError> {
        let response = self.post_form(AUTHORIZE_PATH, "client_id=worldkiln-editor")?;
        if !response.status().is_success() {
            return Err(AuthError::ServiceUnavailable);
        }
        parse_grant(&Self::response_bytes(response)?)
    }

    fn poll(&self, device_code: &str) -> Result<TokenResult, AuthError> {
        if !device_code.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'~')) {
            return Err(AuthError::InvalidResponse);
        }
        let form = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code&client_id={CLIENT_ID}&device_code={device_code}"
        );
        let response = self.post_form(TOKEN_PATH, &form)?;
        let status = response.status();
        let body = Self::response_bytes(response)?;
        if status.is_success() {
            parse_token(&body)
        } else if status == reqwest::StatusCode::BAD_REQUEST {
            parse_error(&body)
        } else {
            Err(AuthError::ServiceUnavailable)
        }
    }

    fn member(&self, access_token: &str) -> Result<MemberIdentity, AuthError> {
        let response = self
            .client
            .get(format!("{MEMBER_ORIGIN}{SESSION_PATH}"))
            .bearer_auth(access_token)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CACHE_CONTROL, "no-store")
            .send()
            .map_err(|_| AuthError::ServiceUnavailable)?;
        if !response.status().is_success() {
            return Err(AuthError::IdentityUnverified);
        }
        parse_member(&Self::response_bytes(response)?)
    }

    fn revoke(&self, access_token: &str) -> Result<(), AuthError> {
        let response = self
            .client
            .post(format!("{MEMBER_ORIGIN}{REVOKE_PATH}"))
            .bearer_auth(access_token)
            .header(reqwest::header::CACHE_CONTROL, "no-store")
            .send()
            .map_err(|_| AuthError::RevocationFailed)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(AuthError::RevocationFailed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    struct MockTransport {
        sequence: RefCell<VecDeque<TokenResult>>,
        polls: Cell<u32>,
        revokes: Cell<u32>,
        identity_ok: bool,
    }

    impl MockTransport {
        fn new(sequence: Vec<TokenResult>) -> Self {
            Self {
                sequence: RefCell::new(sequence.into()),
                polls: Cell::new(0),
                revokes: Cell::new(0),
                identity_ok: true,
            }
        }
    }

    impl DeviceTransport for MockTransport {
        fn begin(&self) -> Result<DeviceGrant, AuthError> {
            parse_grant(br#"{"device_code":"ABCDEFGHIJKLMNOPQRSTUVWXYZ123456","user_code":"WK01-ABCD","verification_uri":"https://aeowun.com/members/device/","expires_in":600,"interval":5}"#)
        }

        fn poll(&self, _code: &str) -> Result<TokenResult, AuthError> {
            self.polls.set(self.polls.get() + 1);
            Ok(self.sequence.borrow_mut().pop_front().unwrap_or(TokenResult::Pending))
        }

        fn member(&self, _token: &str) -> Result<MemberIdentity, AuthError> {
            if !self.identity_ok {
                return Err(AuthError::IdentityUnverified);
            }
            Ok(MemberIdentity {
                id: "stable-member-identifier".to_owned(),
                handle: "example".to_owned(),
                display_name: "Example Member".to_owned(),
            })
        }

        fn revoke(&self, _token: &str) -> Result<(), AuthError> {
            self.revokes.set(self.revokes.get() + 1);
            Ok(())
        }
    }

    fn token() -> TokenResult {
        TokenResult::Authorized {
            access_token: Secret::new("ABCDEFGHIJKLMNOPQRSTUVWXYZ123456".to_owned()).unwrap(),
            expires_in: Duration::from_secs(3600),
        }
    }

    #[test]
    fn browser_code_is_separate_from_hidden_device_secret() {
        let grant = parse_grant(br#"{"device_code":"ABCDEFGHIJKLMNOPQRSTUVWXYZ123456","user_code":"WK01-ABCD","verification_uri":"https://aeowun.com/members/device/","expires_in":600}"#).unwrap();
        assert_eq!(grant.prompt.user_code, "WK01-ABCD");
        assert!(!format!("{:?}", grant.device_code).contains("ABCDEFGHIJKLMNOPQRSTUVWXYZ"));
        assert!(trusted_verification_uri("https://aeowun.com/members/device/"));
        assert!(!trusted_verification_uri("https://aeowun.com.evil.example/members/device/"));
        assert!(!trusted_verification_uri("https://evil.example@aeowun.com/members/device/"));
        assert!(!trusted_verification_uri("http://aeowun.com/members/device/"));
    }

    #[test]
    fn explicit_start_and_throttled_polling() {
        let api = MockTransport::new(vec![TokenResult::Pending, TokenResult::SlowDown, token()]);
        let t0 = Instant::now();
        let mut flow = MemberDeviceLink::default();
        assert_eq!(flow.status(t0), LinkStatus::SignedOut);
        let prompt = flow.begin(&api, t0).unwrap();
        assert_eq!(prompt.user_code, "WK01-ABCD");
        assert_eq!(flow.begin(&api, t0).unwrap_err(), AuthError::AlreadyInProgress);
        assert_eq!(flow.poll(&api, t0).unwrap(), PollProgress::NotDue);
        assert_eq!(api.polls.get(), 0);
        assert_eq!(flow.poll(&api, t0 + Duration::from_secs(5)).unwrap(), PollProgress::Pending);
        assert_eq!(flow.poll(&api, t0 + Duration::from_secs(10)).unwrap(), PollProgress::SlowDown);
        assert_eq!(flow.poll(&api, t0 + Duration::from_secs(19)).unwrap(), PollProgress::NotDue);
        assert_eq!(flow.poll(&api, t0 + Duration::from_secs(20)).unwrap(), PollProgress::Linked);
        assert_eq!(api.polls.get(), 3);
        assert!(matches!(flow.status(t0 + Duration::from_secs(21)), LinkStatus::SignedIn(_)));
        flow.sign_out(&api).unwrap();
        assert_eq!(flow.status(t0 + Duration::from_secs(21)), LinkStatus::SignedOut);
        assert_eq!(api.revokes.get(), 1);
    }

    #[test]
    fn denied_expired_and_cancelled_flows_never_link() {
        let t0 = Instant::now();
        let denied = MockTransport::new(vec![TokenResult::Denied]);
        let mut flow = MemberDeviceLink::default();
        flow.begin(&denied, t0).unwrap();
        assert_eq!(flow.poll(&denied, t0 + Duration::from_secs(5)).unwrap(), PollProgress::Denied);
        assert_eq!(flow.status(t0), LinkStatus::SignedOut);
        flow.begin(&denied, t0).unwrap();
        flow.cancel();
        assert_eq!(flow.status(t0), LinkStatus::SignedOut);
        assert_eq!(denied.polls.get(), 1);

        flow.begin(&denied, t0).unwrap();
        assert_eq!(flow.poll(&denied, t0 + Duration::from_secs(600)).unwrap(), PollProgress::Expired);
        assert_eq!(denied.polls.get(), 1);
    }

    #[test]
    fn verified_identity_is_required_before_linking() {
        let t0 = Instant::now();
        let mut api = MockTransport::new(vec![token()]);
        api.identity_ok = false;
        let mut flow = MemberDeviceLink::default();
        flow.begin(&api, t0).unwrap();
        assert_eq!(flow.poll(&api, t0 + Duration::from_secs(5)).unwrap_err(), AuthError::IdentityUnverified);
        assert_eq!(flow.status(t0), LinkStatus::SignedOut);
        assert_eq!(api.revokes.get(), 1);
    }

    #[test]
    fn rejected_payloads_do_not_create_sessions() {
        assert!(parse_grant(br#"{"device_code":"abc","user_code":"X","verification_uri":"https://aeowun.com/","expires_in":600}"#).is_err());
        assert!(parse_token(br#"{"access_token":"ABCDEFGHIJKLMNOPQRSTUVWXYZ123456","token_type":"basic","expires_in":3600}"#).is_err());
        assert!(parse_member(br#"{"authenticated":false,"member":{"id":"id","handle":"h","displayName":"Member"}}"#).is_err());
        assert!(matches!(parse_error(br#"{"error":"authorization_pending"}"#), Ok(TokenResult::Pending)));
        assert!(matches!(parse_error(br#"{"error":"slow_down"}"#), Ok(TokenResult::SlowDown)));
    }
}
