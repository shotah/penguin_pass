//! The small pages the loopback listener shows in the browser.

use std::io::{Read, Write};

use tiny_http::{Header, Request, Response, StatusCode};

pub(crate) const FORM_PAGE: &str = "\
<!DOCTYPE html>
<html lang=\"en\">
<head>
<meta charset=\"utf-8\">
<title>Penguin Pass — Strava</title>
</head>
<body>
<h1>Connect Strava</h1>
<p>Paste the client ID and client secret from your own Strava API application.
The game does not ask for these on a gamepad.</p>
<ol>
<li>Open <a href=\"https://www.strava.com/settings/api\">https://www.strava.com/settings/api</a>.
Creating an application needs a Strava subscription.</li>
<li>Set Authorization Callback Domain to <code>127.0.0.1</code> and save.
Do not add a port or a path. <code>localhost</code> is also accepted by Strava;
this game uses <code>127.0.0.1</code>.</li>
<li>Paste the client ID and client secret below.</li>
</ol>
<form method=\"post\" action=\"/\">
<p><label>Client ID<br><input name=\"client_id\" required autocomplete=\"off\"></label></p>
<p><label>Client secret<br><input name=\"client_secret\" type=\"password\" required autocomplete=\"off\"></label></p>
<p><button type=\"submit\">Save and connect</button></p>
</form>
</body>
</html>
";

pub(crate) const FORM_AGAIN_PAGE: &str = "\
<!DOCTYPE html>
<html lang=\"en\">
<head>
<meta charset=\"utf-8\">
<title>Penguin Pass — Strava</title>
</head>
<body>
<h1>Connect Strava</h1>
<p>Both the client ID and the client secret are needed.</p>
<p>Set Authorization Callback Domain to <code>127.0.0.1</code> on
<a href=\"https://www.strava.com/settings/api\">https://www.strava.com/settings/api</a>.</p>
<form method=\"post\" action=\"/\">
<p><label>Client ID<br><input name=\"client_id\" required autocomplete=\"off\"></label></p>
<p><label>Client secret<br><input name=\"client_secret\" type=\"password\" required autocomplete=\"off\"></label></p>
<p><button type=\"submit\">Save and connect</button></p>
</form>
</body>
</html>
";

pub(crate) const CONNECTED_PAGE: &str = "\
<!DOCTYPE html>
<html lang=\"en\">
<head>
<meta charset=\"utf-8\">
<title>Penguin Pass — Strava</title>
</head>
<body>
<h1>Strava is connected</h1>
<p>You can close this tab and go back to the game.</p>
</body>
</html>
";

pub(crate) const DENIED_PAGE: &str = "\
<!DOCTYPE html>
<html lang=\"en\">
<head>
<meta charset=\"utf-8\">
<title>Penguin Pass — Strava</title>
</head>
<body>
<h1>Strava was not connected</h1>
<p>Access was declined. You can close this tab. Try again from the game if you want to connect.</p>
</body>
</html>
";

pub(crate) const MISMATCH_PAGE: &str = "\
<!DOCTYPE html>
<html lang=\"en\">
<head>
<meta charset=\"utf-8\">
<title>Penguin Pass — Strava</title>
</head>
<body>
<h1>Connection not accepted</h1>
<p>That reply did not match this connection attempt. Close this tab and try again from the game.</p>
</body>
</html>
";

pub(crate) const FAILED_PAGE: &str = "\
<!DOCTYPE html>
<html lang=\"en\">
<head>
<meta charset=\"utf-8\">
<title>Penguin Pass — Strava</title>
</head>
<body>
<h1>Strava was not connected</h1>
<p>The game could not finish connecting. Close this tab and try again from the game.</p>
</body>
</html>
";

pub(crate) fn html(request: Request, status: u16, body: &'static str) {
    let response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(header(b"Content-Type", b"text/html; charset=utf-8"))
        .with_header(header(b"Cache-Control", b"no-store"));
    let _ = request.respond(response);
}

pub(crate) fn empty(request: Request, status: u16) {
    let response = Response::from_string(String::new()).with_status_code(StatusCode(status));
    let _ = request.respond(response);
}

pub(crate) fn redirect(request: Request, location: &str) {
    let response = Response::from_string("Go back to the game after Strava finishes.\n")
        .with_status_code(StatusCode(303))
        .with_header(header(b"Location", location.as_bytes()))
        .with_header(header(b"Cache-Control", b"no-store"));
    let _ = request.respond(response);
}

pub(crate) fn read_body(request: &mut Request, limit: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = request.as_reader().take(limit).read_to_end(&mut buf);
    buf
}

fn header(name: &[u8], value: &[u8]) -> Header {
    Header::from_bytes(name, value)
        .unwrap_or_else(|_| Header::from_bytes(b"X-Ignored", b"bad-header").expect("static header"))
}

/// A one-line request used to wake a blocked accept.
pub(crate) fn poke(port: u16) {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200))
    else {
        return;
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(200)));
    let _ = stream.write_all(b"GET /poke HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    let mut buf = [0_u8; 32];
    let _ = stream.read(&mut buf);
}
