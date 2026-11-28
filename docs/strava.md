# Strava

`pp-strava` uploads a finished ride to the player's own Strava account. There is no Penguin Pass server. The player creates a Strava API application on Strava's site, the game stores that application's client id and secret on this computer, and a browser login saves a token. After that, a finished FIT file can be uploaded.

The crate does not link Godot. `pp-session` already opens it from `crates/pp-session/src/group.rs` once a settings directory is known. The call list below is the original plan, and it matches that code.

The tests talk only to a mock server on `127.0.0.1`. No request was sent to Strava's API or to its OAuth endpoints. The facts below were read from Strava's developer pages on 21 November 2026.

## What Strava's docs say

Confirmed on these pages:

- [Authentication](https://developers.strava.com/docs/authentication/)
- [Getting started](https://developers.strava.com/docs/getting-started/)
- [Uploading to Strava](https://developers.strava.com/docs/uploads/)
- [Rate limits](https://developers.strava.com/docs/rate-limits/)
- [API reference, createUpload](https://developers.strava.com/docs/reference/)

The authorize page is `GET https://www.strava.com/oauth/authorize`. The query has `client_id`, `redirect_uri`, `response_type=code`, `approval_prompt` (`auto` or `force`; `auto` is the default), `scope`, and an optional `state` that Strava sends back. Scopes are comma-separated or space-separated. `activity:write` is what an upload needs. The token request needs the client secret. The authentication page does not mention PKCE, a code challenge, or a code verifier.

Strava redirects the browser to `redirect_uri`. A refusal is `error=access_denied`. Approval adds `code` and `scope`. `code` is single-use. The token call is described as `POST https://www.strava.com/oauth/token` with `client_id`, `client_secret`, `code`, and `grant_type=authorization_code`. The same page's curl example posts to `https://www.strava.com/api/v3/oauth/token` instead. The getting-started example uses `https://www.strava.com/oauth/token`. This crate uses the prose URL, `https://www.strava.com/oauth/token`. Which of the two paths Strava actually serves was not tried.

The token response has `access_token`, `refresh_token`, `expires_at` (unix seconds), `expires_in`, `scope`, and an `athlete` summary. The getting-started example of that summary includes `firstname` and `lastname`. Access tokens expire six hours after they are issued (`expires_in` is 21600 in the examples). A refresh is `POST` to the same token URL with `grant_type=refresh_token`. Every successful token response includes a refresh token, and it may be a new one. The previous refresh token stops working as soon as a new one is returned. If the access token expires in more than one hour, Strava returns the current access token. If it expires in one hour or less, Strava returns a new one.

`redirect_uri` must be inside the application's Authorization Callback Domain. `localhost` and `127.0.0.1` are allowed. The getting-started page says to set that domain to `localhost` or any domain while building the app. It does not say to include a port or a path.

Upload is `POST https://www.strava.com/api/v3/uploads`, multipart, with `activity:write`. `data_type` is required: `fit`, `fit.gz`, `tcx`, `tcx.gz`, `gpx`, `gpx.gz`, or `json` (json is only for strength training). The file field is `file`. Optional fields include `name`, `description`, `external_id`, `sport_type`, and `trainer`. `external_id` defaults to the file name and should be unique. `sport_type=VirtualRide` overrides the type in the file. `trainer` is an integer; `1` forces a stationary activity. A file with no latitude or longitude is marked stationary on its own. The upload guide's curl example also sends `trainer=0`. The API reference's `createUpload` parameter table, as fetched the same day, lists `trainer` and does not list `sport_type`. The upload guide does list `sport_type`, including `VirtualRide`.

The upload is asynchronous. A success is HTTP 201 while the activity is still processing. Poll `GET /api/v3/uploads/{id}` until `activity_id` is set or `error` is set. Strava asks for a poll interval of one second or longer. The mean processing time is under two seconds. The documented duplicate error is a string such as `Test_Walk.gpx duplicate of activity 21234316`. Error text may include HTML. The activity id is a 64-bit integer.

A new application can authenticate one athlete (single-player mode). A Strava subscription is required to create an application. The default overall limit is 200 requests per 15 minutes and 2,000 per day. The default limit on everything except uploads is 100 per 15 minutes and 1,000 per day. Over the limit, the response is HTTP 429. Upgrading the application in the dashboard raises the athlete cap to 10.

## What was not confirmed

These were not on the pages above, or the pages disagreed, and nothing was sent to Strava to check.

- Whether every port is accepted. The callback domain is a domain, and the getting-started example uses `http://localhost/exchange_token` with no port. This crate listens on `127.0.0.1`, port 53682, then 53683 through 53686, then any free port. The redirect is `http://127.0.0.1:{port}/callback`.
- Whether `activity:write` alone puts the athlete's name in the token response. The only published example that shows `firstname` and `lastname` requested `scope=read`. `read` is the scope described as public profile data. This crate requests `activity:write,read`. If the athlete turns off upload permission, the login is not saved.
- Which token URL is the one Strava serves. See above. This crate posts to `/oauth/token`.
- The activity page `https://www.strava.com/activities/{id}`. The upload response has `activity_id` and no URL. The crate builds that website address from the id.
- Whether `sport_type=VirtualRide` together with `trainer=1` is how Strava shows a FIT file that has no GPS. Both fields are documented. `VirtualRide` sets the sport. `trainer=1` forces the stationary flag that a file with no position gets anyway. This crate sends both. It was not tried against Strava.
- PKCE is absent from the authentication page, which is not a promise that a later page will not add it.

## What the player does

1. A Strava subscription is required to create an API application. Log in at <https://www.strava.com>.
2. Open <https://www.strava.com/settings/api>.
3. Create an application if you do not have one. The name can be Penguin Pass. The website, description, and category can be whatever Strava accepts. A new application can log in only the athlete who created it.
4. Set Authorization Callback Domain to `127.0.0.1` and save. Do not add a port or a path. Strava also allows `localhost`. This game uses `127.0.0.1`.
5. Copy the Client ID and the Client Secret from that page. The secret stays on this computer.
6. In the game, choose Connect Strava. A browser tab opens.
7. If the game does not already have the keys, the tab is a small form. Paste the Client ID and the Client Secret and submit. The form links to the API settings page and repeats the callback domain. Typing the secret with a gamepad is not required.
8. Strava asks you to approve the application. Allow uploads and the read permission. The read permission is so the game can show your name.
9. The tab says Strava is connected and that you can close it. The game shows your name.
10. When a ride is saved as a FIT file, the game uploads it. The same file is not sent again. On Strava it is a virtual ride.

If you decline, try again from the game. Disconnect in the game forgets the saved login and keeps the keys. Strava's own settings still list the application until you remove it there. This crate does not call Strava's revoke endpoint.

One person uploading their own rides stays under the default rate limit.

## What is stored

One TOML file. The session chooses the path. The place to use is `strava.toml` next to the settings file.

The file holds the client id, client secret, access token, refresh token, expiry, athlete name, and each uploaded file name with its activity id. It is written to a temporary name in the same directory, flushed, and renamed over the destination. The mode is `0600`.

A missing file is no keys, and it is not created until something is saved. A file that does not parse is no keys. A warning is printed, and the bytes are left as they are. The crate does not panic.

`Debug` output, events, error strings, and that warning do not contain the client secret, the access token, or the refresh token. The file on disk does.

## What an upload does

The access token is refreshed when it expires within five minutes. The new refresh token is saved before the file is sent, because Strava invalidates the old one immediately.

The file goes up once, as `data_type=fit`, with `name`, `description`, `external_id` set to the file name, `sport_type=VirtualRide`, and `trainer=1`. Strava's processing is polled, starting at one second and backing off to at most eight seconds, for up to two minutes.

The result is the activity id and `https://www.strava.com/activities/{id}`, or a failure. A duplicate error is a failure with `duplicate` set, and the file name is still recorded as uploaded. The activity id is the number in `duplicate of activity`, or 0 if that number is not there.

HTTP 401 on the upload causes one refresh and one more upload. A 401 from that refresh, or a 401 from the upload after the refresh, drops the tokens and the state becomes not connected. The keys stay. HTTP 429 is reported as the rate limit and is not retried. Each request times out after 20 seconds. There is no retry loop.

`was_uploaded` matches the file name, not the directory.

## What the session calls

Already applied in `crates/pp-session/src/group.rs`. The block below is the original plan.

On the session, next to the rides directory:

```text
strava: Option<pp_strava::StravaLink>
```

Open it when the settings path is known. `dir` is the directory of `settings.toml`.

```text
strava = StravaLink::open(dir.join("strava.toml"), Endpoints::strava())
```

Every method returns at once. A background thread does the waiting. Each frame:

```text
for event in strava.drain_events() { show it }
```

Connect:

```text
if let Some(url) = strava.begin_connect() { open url in the browser }
```

With keys, `url` is Strava's authorize page. Without keys, `url` is the local form. The listener waits five minutes. `cancel_connect` stops it and leaves the saved keys and tokens. `disconnect` forgets the tokens and keeps the keys.

After a FIT file is written, if the state is connected and `was_uploaded` is false:

```text
strava.upload(path, UploadMeta { name, description })
```

`name` is the course title. `description` can be empty. The events are `UploadStarted`, then `Uploaded` or `UploadFailed`.

`state` is what the menu can show: no keys, not connected, connecting (with the URL), connected (with the athlete's name), or an error opening the local page.

Dropping the link stops the thread and the listener. A request already in flight runs until its 20 second timeout.

## Dependencies

`ureq` 2.12 (MIT OR Apache-2.0) is the blocking HTTP client. Its `tls` feature uses rustls, so TLS does not use OpenSSL. The upload body is built in this crate and sent with a `Content-Length`, which a small loopback server can read without waiting for the connection to close. `tiny_http` 0.12 (MIT OR Apache-2.0), without its default features, is the loopback listener and the mock server in the tests. `form_urlencoded` (MIT OR Apache-2.0) encodes the authorize query and the key form. `getrandom` 0.2 (MIT OR Apache-2.0) supplies the OAuth `state` value and the multipart boundary. `serde`, `serde_json`, and `toml` match the other crates.

`ureq`'s tls feature pulls `ring` (Apache-2.0 AND ISC) and `webpki-roots`. `webpki-roots` 0.26.9 and later are CDLA-Permissive-2.0, which is outside the licence set this repo allows. The crate pins `webpki-roots` to 0.26.8, which is MPL-2.0. The client does not read `HTTP_PROXY`.

`./x licenses` records the crates linked into `pp-godot`, including `pp-strava`. Re-run `./x licenses` after a dependency change.
