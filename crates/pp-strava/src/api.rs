//! HTTP calls to the Strava origin. The origin is injectable so tests never
//! leave the machine.

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::Endpoints;

pub(crate) const SCOPE: &str = "activity:write,read";
const BODY_LIMIT: u64 = 64 * 1024;

pub(crate) enum ApiError {
    Timeout,
    NotFinished,
    Transport,
    Unreadable,
    Status(u16),
    BadResponse,
    RateLimited,
    Unauthorized,
    Duplicate { activity_id: u64 },
    Rejected,
}

pub(crate) enum UploadProgress {
    Pending { id: u64 },
    Ready { activity_id: u64 },
}

pub(crate) struct TokenSet {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at: u64,
    pub(crate) athlete_name: Option<String>,
    pub(crate) scope: Option<String>,
}

pub(crate) fn authorize_url(
    endpoints: &Endpoints,
    client_id: &str,
    redirect: &str,
    state: &str,
) -> String {
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("response_type", "code")
        .append_pair("approval_prompt", "auto")
        .append_pair("scope", SCOPE)
        .append_pair("state", state)
        .finish();
    format!("{}/oauth/authorize?{query}", origin(&endpoints.oauth_base))
}

pub(crate) fn activity_url(activity_id: u64) -> String {
    format!("https://www.strava.com/activities/{activity_id}")
}

pub(crate) fn exchange_code(
    endpoints: &Endpoints,
    timeout: Duration,
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> Result<TokenSet, ApiError> {
    let agent = agent(timeout);
    let response = agent.post(&token_url(endpoints)).send_form(&[
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code", code),
        ("grant_type", "authorization_code"),
    ]);
    let (status, body) = call(response)?;
    token_from(status, &body)
}

pub(crate) fn refresh(
    endpoints: &Endpoints,
    timeout: Duration,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<TokenSet, ApiError> {
    let agent = agent(timeout);
    let response = agent.post(&token_url(endpoints)).send_form(&[
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ]);
    let (status, body) = call(response)?;
    token_from(status, &body)
}

pub(crate) fn create_upload(
    endpoints: &Endpoints,
    timeout: Duration,
    access_token: &str,
    file: &Path,
    external_id: &str,
    name: &str,
    description: &str,
) -> Result<UploadProgress, ApiError> {
    let file_bytes = std::fs::read(file).map_err(|_| ApiError::Unreadable)?;
    let (content_type, bytes) = multipart(
        &[
            ("name", name),
            ("description", description),
            ("data_type", "fit"),
            ("external_id", external_id),
            ("sport_type", "VirtualRide"),
            ("trainer", "1"),
        ],
        external_id,
        &file_bytes,
    );
    let agent = agent(timeout);
    let auth = format!("Bearer {access_token}");
    let response = agent
        .post(&uploads_url(endpoints))
        .set("Authorization", &auth)
        .set("Content-Type", &content_type)
        .send_bytes(&bytes);
    let (status, body) = call(response)?;
    classify_upload(status, &body)
}

pub(crate) fn poll_upload(
    endpoints: &Endpoints,
    timeout: Duration,
    access_token: &str,
    upload_id: u64,
) -> Result<UploadProgress, ApiError> {
    let agent = agent(timeout);
    let auth = format!("Bearer {access_token}");
    let url = format!("{}/{}", uploads_url(endpoints), upload_id);
    let response = agent.get(&url).set("Authorization", &auth).call();
    let (status, body) = call(response)?;
    classify_upload(status, &body)
}

pub(crate) fn scope_allows_upload(scope: Option<&str>) -> bool {
    let Some(scope) = scope else {
        return true;
    };
    scope
        .split(|c: char| c.is_whitespace() || c == ',')
        .any(|part| part == "activity:write")
}

pub(crate) fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn token_url(endpoints: &Endpoints) -> String {
    format!("{}/oauth/token", origin(&endpoints.oauth_base))
}

fn uploads_url(endpoints: &Endpoints) -> String {
    format!("{}/api/v3/uploads", origin(&endpoints.api_base))
}

fn origin(base: &str) -> &str {
    base.trim_end_matches('/')
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(timeout)
        .timeout_connect(timeout)
        .redirects(0)
        .build()
}

fn call(result: Result<ureq::Response, ureq::Error>) -> Result<(u16, String), ApiError> {
    match result {
        Ok(response) => Ok(read_response(response)),
        Err(ureq::Error::Status(status, response)) => Ok(read_response_status(status, response)),
        Err(ureq::Error::Transport(err)) if transport_timed_out(&err) => Err(ApiError::Timeout),
        Err(ureq::Error::Transport(_)) => Err(ApiError::Transport),
    }
}

fn transport_timed_out(err: &ureq::Transport) -> bool {
    err.to_string().to_ascii_lowercase().contains("timed out")
}

fn read_response(response: ureq::Response) -> (u16, String) {
    read_response_status(response.status(), response)
}

fn read_response_status(status: u16, response: ureq::Response) -> (u16, String) {
    let mut text = String::new();
    let _ = response
        .into_reader()
        .take(BODY_LIMIT)
        .read_to_string(&mut text);
    (status, text)
}

fn multipart(fields: &[(&str, &str)], file_name: &str, file_bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = multipart_boundary();
    let mut body = Vec::new();
    for (name, value) in fields {
        let _ = write!(
            body,
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        );
    }
    let safe_name = file_name.replace(['"', '\\', '\r', '\n'], "_");
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    body.extend_from_slice(file_bytes);
    let _ = write!(body, "\r\n--{boundary}--\r\n");
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn multipart_boundary() -> String {
    let mut bytes = [0_u8; 8];
    let _ = getrandom::getrandom(&mut bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::from("ppstrava");
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

fn token_from(status: u16, body: &str) -> Result<TokenSet, ApiError> {
    if status == 401 {
        return Err(ApiError::Unauthorized);
    }
    if status == 429 {
        return Err(ApiError::RateLimited);
    }
    if !(200..300).contains(&status) {
        return Err(ApiError::Status(status));
    }
    let parsed: TokenJson = serde_json::from_str(body).map_err(|_| ApiError::BadResponse)?;
    if parsed.access_token.is_empty() || parsed.refresh_token.is_empty() {
        return Err(ApiError::BadResponse);
    }
    Ok(TokenSet {
        access_token: parsed.access_token,
        refresh_token: parsed.refresh_token,
        expires_at: expiry(parsed.expires_at, parsed.expires_in),
        athlete_name: parsed.athlete.map(|athlete| athlete.display_name()),
        scope: parsed.scope,
    })
}

fn expiry(expires_at: Option<u64>, expires_in: Option<u64>) -> u64 {
    if let Some(at) = expires_at.filter(|at| *at > 0) {
        return at;
    }
    let Some(secs) = expires_in else {
        return 0;
    };
    now_unix().saturating_add(secs)
}

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn classify_upload(status: u16, body: &str) -> Result<UploadProgress, ApiError> {
    if status == 401 {
        return Err(ApiError::Unauthorized);
    }
    if status == 429 {
        return Err(ApiError::RateLimited);
    }
    if status == 408 {
        return Err(ApiError::Timeout);
    }
    let parsed = serde_json::from_str::<UploadJson>(body).ok();
    if let Some(error) = parsed.as_ref().and_then(|item| item.error.as_deref())
        && !error.is_empty()
    {
        return Err(error_from_upload(error));
    }
    if !(200..300).contains(&status) {
        return Err(ApiError::Status(status));
    }
    let Some(parsed) = parsed else {
        return Err(ApiError::BadResponse);
    };
    if let Some(activity_id) = parsed.activity_id {
        return Ok(UploadProgress::Ready { activity_id });
    }
    let Some(id) = parsed.id else {
        return Err(ApiError::BadResponse);
    };
    Ok(UploadProgress::Pending { id })
}

fn error_from_upload(error: &str) -> ApiError {
    if error.to_ascii_lowercase().contains("duplicate") {
        ApiError::Duplicate {
            activity_id: duplicate_activity_id(error).unwrap_or(0),
        }
    } else {
        ApiError::Rejected
    }
}

fn duplicate_activity_id(error: &str) -> Option<u64> {
    let lower = error.to_ascii_lowercase();
    let start = lower.find("activity ")? + "activity ".len();
    let digits: String = error[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

#[derive(Deserialize)]
struct TokenJson {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    expires_at: Option<u64>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    athlete: Option<AthleteJson>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct AthleteJson {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    firstname: Option<String>,
    #[serde(default)]
    lastname: Option<String>,
}

impl AthleteJson {
    fn display_name(&self) -> String {
        let joined = format!(
            "{} {}",
            self.firstname.as_deref().unwrap_or(""),
            self.lastname.as_deref().unwrap_or("")
        );
        let joined = joined.trim();
        if !joined.is_empty() {
            return joined.to_owned();
        }
        if let Some(username) = self.username.as_deref().filter(|name| !name.is_empty()) {
            return username.to_owned();
        }
        if let Some(id) = self.id {
            return format!("Athlete {id}");
        }
        "Strava athlete".to_owned()
    }
}

#[derive(Deserialize)]
struct UploadJson {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    activity_id: Option<u64>,
}
