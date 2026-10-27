# FOQA Challenge Protocol v1 (design)

An open, versioned HTTP protocol through which **any** website can publish
landing challenges ([challenges.md](challenges.md)) for FOQA-Sim to fly,
and receive the flown attempts to score and rank. FOQA-Sim owns this
protocol and publishes it in this repository (public with FOQA-Sim); a
site depends on the protocol, its example fixtures (A11) and the open
`fdm challenge` command, never on FOQA-Sim internals. The first
implementer is Pilot Aptitude (Mollymawk `docs/landing-challenges.md`),
but nothing below names it.

Status: design. Part A is the wire protocol (what a site implements).
Part B is the app side (how FOQA-Sim implements the client without
depending on it). Revised 2026-10-27 after a security and a UX review.
Built (2026-10-28): slices F1 (marker, slice, golden), F3, F4
(`crates/connect`, `app/src-tauri/src/connect.rs`, Challenges → Sites) and
F5 (deep links, the in-sim strip and EFB Challenges tab, toasts); see
"Implementation notes" at the end. F6 is not built.

## Principles

1. **FOQA-Sim is complete without any site.** Recording, debriefs, the
   challenge definition, placing a start and grading an attempt all work
   offline, from a challenge JSON file. The protocol only adds "where do
   challenges come from" and "where do attempts go".
2. **Optional, removable module.** The client lives in one crate and one
   app feature (`connect`). No core crate (`fdm-core`, `capture`,
   `recorder`, `analyzer`, `rules`, `debrief`, `challenge`, `cli`) and no
   core app module depends on it; the app builds and runs with it off.
3. **No site baked in.** No URL, name or key of any site in the code or
   the build. Sites are data the user adds (by typing an address or by
   confirming a deep link) and can remove.
4. **The site trusts nothing from the client except the bytes.** It
   re-scores the uploaded recording with `fdm challenge`; the client's own
   result is display only.
5. **Least data.** Only the attempt's slice of one flight is ever uploaded,
   and only for an attempt the site issued while online.
6. **Secrets stay out of URLs, logs and synced folders.** Tokens live in
   memory and the OS credential store; nothing secret is in a path, a
   query string, a log line or under `Documents`.

---

# Part A: the protocol

## A1. Discovery

A site is identified by its **origin** (`https://host[:port]`, no path).
The user adds one in the app (Challenges → Sites → Add) or confirms one
from a deep link (A3). The app then fetches:

`GET <origin>/.well-known/foqa-challenges.json`

```json
{
  "protocol": "foqa-challenges",
  "versions": [1],
  "name": "Pilot Aptitude",
  "api": { "1": "https://pilotaptitude.example/api/foqa/v1" },
  "auth": {
    "authorize": "https://pilotaptitude.example/#/foqa/authorize",
    "device_verification": "https://pilotaptitude.example/#/device"
  },
  "links": {
    "challenge": "https://pilotaptitude.example/#/challenges/{id}",
    "attempt": "https://pilotaptitude.example/#/challenges/{id}/attempts/{attempt}",
    "account": "https://pilotaptitude.example/#/account"
  },
  "limits": { "max_upload_bytes": 2097152 }
}
```

Rules:

- Served with `Content-Type: application/json`, at most 8 KiB, over
  `https` to a DNS host name. IP literals are refused; plain `http` and
  loopback (`localhost`, `127.0.0.1`) are accepted only with the app's
  developer setting "Allow local sites" (off by default).
- **Every URL in it must be on the same origin as the site.** The app
  refuses the document otherwise, so a site cannot point the app at a
  third host.
- `versions`: the protocol versions served. The app uses the highest
  version both sides support; none in common → "This site needs a newer
  (or older) FOQA-Sim."
- `name` is shown next to the origin, never instead of it (it is
  self-declared). Max 60 characters, rendered as text. The origin is
  always shown with its host in ASCII (IDNA punycode, `xn--…`), so a
  look-alike host is visible.
- `links` are templates for "View on site" buttons; `{id}` and `{attempt}`
  are percent-encoded by the app. Optional: without them the app shows no
  site links.
- `limits.max_upload_bytes`: the app does not upload a larger slice and
  says why. Default 2 MiB when absent.
- The app caches the document per site for 24 h and refetches it on any
  `404`/`unsupported_version` from the API.

## A2. Identity: sign-in per site

Each site has its own accounts. The app holds one grant per site; scores
belong to the account that approved it. Two flows, same tokens.

### A2.1 Primary: browser + loopback redirect (RFC 8252 + PKCE, RFC 7636)

The user is usually already signed in on the site, so it is one "Allow"
click.

1. The app binds an HTTP listener on **`127.0.0.1`, a random free port**
   (never `0.0.0.0`, never `localhost`, RFC 8252 §8.3), generates `state`
   (128 bit) and a PKCE `code_verifier` (43–128 chars, RFC 7636 §4.1) and
   opens the system browser (never an embedded webview, RFC 8252 §8.12)
   at `auth.authorize` with query
   `client_id=foqa-sim&redirect_uri=http://127.0.0.1:<port>/callback&state=<state>&code_challenge=<S256>&code_challenge_method=S256&client_name=FOQA-Sim%200.1.2`.
2. The site signs the user in if needed and shows "Allow FOQA-Sim on this
   PC to fly challenges as **<handle>**?" On Allow it redirects to
   `redirect_uri?code=<code>&state=<state>`; on Deny,
   `?error=access_denied&state=<state>`.
3. The listener accepts **one** request: method `GET`, path exactly
   `/callback`, `Host` exactly `127.0.0.1:<port>` (DNS rebinding), and
   `state` equal to the one sent (constant-time compare) **for both
   `code=` and `error=` responses**, so another local process cannot
   cancel a sign-in. Anything else gets 400 and the listener keeps
   waiting. It answers with a static "You can return to FOQA-Sim" page
   (no scripts, no external resources), then closes. Timeout 5 minutes;
   closed on cancel.
4. The app exchanges the code: `POST {api}/token`
   `{"grant_type":"authorization_code","code":…,"code_verifier":…,"redirect_uri":…,"client_id":"foqa-sim"}`
   → `TokenResponse` (A2.3).

The site must: accept only `redirect_uri` of the exact form
`http://127.0.0.1:<1024–65535>/callback` (any port, RFC 8252 §7.3),
require `S256`, bind the code to the user, the `code_challenge` and the
redirect URI, make it single use and valid for 60 s.

### A2.2 Fallback: device authorization (RFC 8628)

When the loopback listener cannot bind or the browser is on another
device. `POST {api}/device/code` `{"client_id":"foqa-sim","client_name":"FOQA-Sim 0.1.2"}`
→ RFC 8628 §3.2 response (`device_code`, `user_code`, `verification_uri`,
`verification_uri_complete`, `expires_in`, `interval`). The app shows the
code and opens `verification_uri_complete`; it polls `POST {api}/token`
`{"grant_type":"urn:ietf:params:oauth:grant-type:device_code","device_code":…,"client_id":"foqa-sim"}`
honouring `interval`, `slow_down` (+5 s), `authorization_pending`,
`access_denied`, `expired_token` (RFC 8628 §3.5).

### A2.3 Tokens

```json
{ "access_token": "…", "token_type": "Bearer", "expires_in": 3600,
  "refresh_token": "…", "user": { "id": "…", "handle": "ege" } }
```

- **Access token:** opaque, short-lived (≤ 1 h), `Authorization: Bearer`.
  The app keeps it in memory only.
- **Refresh token:** opaque, long-lived (site policy, e.g. 180 days
  sliding). `POST {api}/token` `{"grant_type":"refresh_token","refresh_token":…,"client_id":"foqa-sim"}`
  returns a **new** pair each time (rotation). Reuse of a rotated token is
  treated as theft and revokes the whole grant (RFC 9700 §4.14.2), with
  one **grace rule** against false positives (a response lost in transit,
  an app killed mid-refresh): if the presented token is the *previous*
  refresh token, it was rotated out **≤ 60 s** ago, and the pair issued
  by that rotation has **not been used yet**, the site rotates again and
  returns a fresh pair (the unused one dies) instead of revoking.
- **Client side:** refresh is **single-flight** per site in
  `crates/connect`: concurrent callers wait for the one refresh in
  progress. The new refresh token is written to the credential store
  before the access token is used.
- `POST {api}/token/revoke` (Bearer) revokes the grant: the app's
  "Sign out" and "Remove site".
- Token bodies are JSON (a deliberate deviation from RFC 6749's form
  encoding, to keep one body format); error bodies on `/token` use the RFC
  error codes in `error` (A6).
- The client never parses tokens. 401 `invalid_token` on an API call →
  one refresh → still 401 → the site is shown as signed out; pending
  uploads wait (B6).

## A3. Deep link

The site's "Fly this challenge" button links to:

```
foqa-sim://challenge?site=<origin>&id=<challenge id>[&v=<version>]
foqa-sim://connect?site=<origin>
```

`site` is percent-encoded. The URL is **untrusted input** (any web page
can open it):

- Total length ≤ 512; scheme exactly `foqa-sim`; host exactly
  `challenge` or `connect`; no userinfo, path other than empty or `/`,
  or fragment; unknown query keys ignored; a duplicated key rejects.
- `site` must parse as an origin under A1's rules (https, DNS host, no IP
  literal, no path/query/fragment, no trailing dot; `localhost` and names
  under it only with the developer setting, like loopback).
- `id`: `^[a-z0-9][a-z0-9-]{2,63}$`; `v`: integer 1–100000.
- A link **never acts by itself**: no fetch, no library write, no
  navigation and no placing before a click in the app. It only brings the
  window up with a question. If `site` is not a connected site, the app
  shows "Connect to **<origin in ASCII>**?" and fetches nothing from it
  before the user confirms; Connect adds the site, starts the sign-in and
  then opens **the clicked challenge's briefing** (the pending link is
  kept in memory for 10 minutes, never on disk). A connected site that is
  signed out asks "Sign in to <host>?" ("Sign in and open"); a signed-in
  one asks "Open <id> from <host>?" and fetches that challenge (A4) only
  on Open. Before the click the question shows only what the link says
  (host, id, version); the title comes from the site after it. Placing
  the aircraft and uploading still need the user.
- A link with `v` opens only that version: when the site has another, the
  app says so ("<host> has a newer version of this challenge…") and takes
  nothing. A challenge already in the library under the same id and
  version with other bytes is never overwritten silently: the app asks
  "Replace your copy of “<title>”?" (Keep mine opens the copy kept).
- One link is asked about at a time: a link arriving meanwhile waits (the
  newest one only) and is asked about after this one is closed; the same
  link arriving again while it waits is not queued twice. A link is given
  to the window once: a reload drops it rather than asking again, and
  whatever a link did needs a new click to happen again.
- Deep links can be turned off in settings (B3); the app then ignores
  them.

## A4. API

All paths are relative to `api["1"]`. JSON in snake_case, UTF-8. Times
RFC 3339 UTC. Every request carries `User-Agent: FOQA-Sim/<version>`,
`X-Foqa-Protocol: 1`, `X-Foqa-Version: <app version>`; authenticated ones
`Authorization: Bearer <access token>`. The app never sends `Origin`.
Every response carries `X-Request-Id`, which the app writes into its
logs (B5) so both sides' logs line up. **No path or query ever carries a
secret:** attempts are addressed by their id, authorised by the Bearer
token and ownership.

| Call | Auth | Body → Response |
|---|---|---|
| `GET /me` | Bearer | → `{"user":{"id","handle"}}`; no email |
| `GET /challenges` | optional | → `{"challenges":[ChallengeEntry]}`: open and upcoming, newest version of each |
| `GET /challenges/{id}[?version=n]` | optional | → `{"challenge":ChallengeEntry}` |
| `POST /challenges/{id}/attempts` | Bearer | `{"version":n,"client_attempt_id":uuid}` → 201 `{"attempt":IssuedAttempt,"definition":{…}}`; **idempotent**: the same `client_attempt_id` from the same user returns 200 with the existing attempt (same challenge and version, else 409 `conflict`) |
| `POST /attempts/{id}/abandon` | Bearer, owner | → 200 `{"attempt":AttemptStatus}`; `issued` → `abandoned`; idempotent; frees an open-attempt slot |
| `PUT /attempts/{id}/recording` | Bearer, owner | `application/octet-stream`, the slice (A5); header `X-Foqa-Sha256` (hex) → 202 `{"attempt":AttemptStatus}` |
| `PUT /attempts/{id}/client-result` | Bearer, owner | the app's `Attempt` JSON, ≤ 16 KiB → 204; display only |
| `GET /attempts/{id}` | Bearer, owner | → `{"attempt":AttemptStatus}` |
| `GET /me/attempts[?challenge=id&before=ts&limit=50]` | Bearer | → `{"attempts":[AttemptStatus],"next":ts\|null}` |
| `POST /token`, `POST /token/revoke`, `POST /device/code` | A2 | A2 |

Types:

```jsonc
// ChallengeEntry
{ "id": "ltfj-06-gusty-2026w44", "version": 1, "title": "…",
  "state": "open" | "upcoming" | "closed",
  "opens_at": "…" | null, "closes_at": "…" | null,
  "definition_sha256": "hex",   // of the exact definition bytes below
  "definition": { /* the challenge JSON of challenges.md, byte for byte
                     what the server scores against */ } }

// IssuedAttempt
{ "id": "uuid", "ref": "22-char base64url",   // marker ref (A7), opaque
  "challenge_id": "…", "challenge_version": 1,
  "definition_sha256": "hex",
  "issued_at": "…",
  "fly_within_s": 1800,     // the flight must start within this, from the response
  "upload_within_s": 86400, // the recording is accepted within this
  "fly_by": "…", "upload_by": "…" }   // the same, absolute; display only

// AttemptStatus
{ "id": "uuid", "challenge_id": "…", "challenge_version": 1,
  "status": "issued" | "queued" | "scoring" | "scored" | "rejected"
          | "failed" | "expired" | "abandoned" | "removed",
  "issued_at": "…", "uploaded_at": "…" | null, "scored_at": "…" | null,
  "result": { /* fdm challenge output, the server's */ } | null,
  "verified": false,
  "unverified": ["marker_mismatch"],   // reason codes (A6), [] when verified
  "reject": { "code": "…", "detail": "…" } | null,
  "rank": 12 | null,
  "url": "https://…" | null }   // the site's page for this attempt
```

Semantics:

- The app flies **exactly** the `definition` returned by the attempt
  call, after `Challenge::parse` accepts it; it keeps the definition's
  bytes as received, checks them against `definition_sha256` and logs
  both. A definition this app cannot parse is listed as "needs a newer
  FOQA-Sim" and cannot be started.
- **Clocks:** the client never compares its clock with the server's. It
  computes its deadlines as *response received* + `fly_within_s` /
  `upload_within_s`; `fly_by`/`upload_by` are for display.
- **Status:** the first accepted upload moves `issued` → `queued`. The
  server moves `issued` → `expired` only when `upload_by` passes with
  nothing uploaded; `fly_by` is enforced by the client (it refuses to
  place after it) and checked by the server through the duration term of
  "verified" (A10.5), since the server cannot see when a flight started.
- **Retries:** an upload whose `X-Foqa-Sha256` equals the stored one
  returns 202 with the current status **without the body being read**, in
  any status after `issued`, so a retry after a lost response is safe.
  Different bytes on an attempt past `issued` → 409 `conflict`.
- **Final statuses:** `scored`, `rejected`, `failed`, `expired`,
  `abandoned`, `removed`. `scored` stays final; `verified`, `unverified`
  and `rank` may still change (moderation, a better attempt of the same
  user). `removed` is a moderation overlay: the site reports it instead of
  the scoring status while the attempt is removed.
- **Forward compatibility:** a status the client does not know is **not
  final**: it keeps polling on the normal schedule and shows "Waiting for
  the site".
- **Superseded versions:** issuing needs the newest version
  (`version_superseded`), but an attempt already issued for an older
  version keeps uploading until its `upload_by` and ranks on that
  version's board.
- **Open attempts:** a site may cap open attempts per user (`issued`
  with `fly_by` in the future). The app abandons its own stale ones (B6).
- **Polling:** every 2 s for 60 s after upload, then every 30 s for
  10 min, then at app start and when the Challenges screen opens, until a
  final status.

## A5. The recording slice

What is uploaded: a **compact flight v1** file ([format.md](format.md),
magic `FQJC`) cut from the flight that contains the attempt:

- Samples from 10 s before the attempt's begin marker (A7) to 5 s after
  its end marker, plus, for every parameter, its last sample before that
  window (so values at the window start are known). Sim times are **not**
  rebased.
- The airport snapshot of the challenge's airport (and of any airport
  recorded inside the window): `fdm slice` does not know the challenge, so
  it keeps the latest earlier snapshot of every airport within 10 km of a
  position flown inside the window, which includes the challenge's runway
  ([format.md](format.md), Recording slice); the aircraft identity in force at the
  window start and any change inside it, with `atc_id` (tail number) and
  `title` (the sim title, which can carry a user-named livery) blanked;
  `atc_model`/`atc_type` kept for the aircraft check; sim events inside
  the window.
- Dropped: the flight-loaded event (its text is a local `.FLT` path that
  can contain the Windows user name), pilot report notes, everything
  outside the window.

The site must refuse anything that does not start with `FQJC` or exceeds
its `max_upload_bytes`.

## A6. Errors, statuses and their copy

Every non-2xx response is `{"error":"<code>","message":"<human text>"}`
(plus `retry_after` seconds when relevant; also the `Retry-After`
header). The app logs both, acts on `error`, and shows **its own** plain
copy below (the site's `message` is shown under "Details"). This table is
the single source for both sides; Mollymawk's design uses the same codes
and statuses.

| HTTP | `error` | Retry | App behaviour | Copy shown |
|---|---|---|---|---|
| 400 | `invalid_request` | no | attempt `upload_refused` | "The site couldn't read this request. Update FOQA-Sim, or report it to the site." |
| 400 | `bad_recording` | no | `upload_refused` | "The site couldn't read this recording." |
| 400 | `checksum_mismatch` | yes, 3× | re-read slice, retry; then `upload_refused` | "The upload was damaged on the way. Retrying…" |
| 401 | `invalid_token` | refresh once | else signed out; uploads wait | "Signed out of <site>. Sign in again to upload." |
| 403 | `handle_required` | no | open `links.account` on click | "Pick a display name on <site> first, then try again." |
| 403 | `forbidden` | no | — | "<site> doesn't allow this for your account." |
| 403 | `bad_origin` | no | log; something on the path added an `Origin` header | "Your network changed the request. Try another network." |
| 404 | `not_found` | no | challenge/attempt gone | "This challenge or attempt is no longer on <site>." |
| 409 | `conflict` | no | — | "This attempt already has a different recording." |
| 409 | `duplicate_recording` | no | — | "This recording was already uploaded for another attempt." |
| 409 | `challenge_closed` | no | — | "This challenge is closed." |
| 409 | `version_superseded` | no | refetch the challenge | "This challenge was updated. Open it again to fly the new version." |
| 410 | `attempt_expired` | no | keep local result; `expired` | "Too late to upload: the window closed at <time>. Your local grade is kept." |
| 413 | `too_large` | no | — | "This recording is larger than <site> accepts (<size>)." |
| 426 | `unsupported_version` | no | refetch well-known | "<site> needs a newer FOQA-Sim. Update to continue." |
| 429 | `rate_limited`, `busy` | after `retry_after` | — | "<site> is busy. Retrying in <n> s." |
| 429 | `too_many_open_attempts` | after abandoning stale ones (B6) | — | "You have unfinished attempts. Finish or abandon one, then fly again." |
| 5xx, network | any | backoff (B6) | — | "Can't reach <site>. Will retry until <upload_by>." |
| other 4xx | unknown | no | — | "<site> refused: <message>" |
| `/token` 400 | `authorization_pending`, `slow_down`, `access_denied`, `expired_token`, `invalid_grant` | RFC 8628 / 6749 | — | "Sign-in was cancelled or expired. Try again." |

Statuses (`AttemptStatus.status`), reject codes and unverified reasons:

| Value | Kind | Copy shown |
|---|---|---|
| `issued` | status | "Waiting for your flight (fly within <mm> min)" |
| `queued`, `scoring` | status | "Uploaded. Scoring…" |
| `scored` | status, final | "Scored <total>" + rank, or + "not ranked" with the reasons below |
| `rejected` | status, final | by `reject.code` below |
| `failed` | status, final | "The site couldn't score this attempt. It's their side; try again later." |
| `expired` | status, final | "Not uploaded in time." |
| `abandoned` | status, final | "Abandoned." |
| `removed` | status, final | "Removed by the site's moderators." |
| unknown | status, not final | "Waiting for the site" |
| `bad_recording` | reject | "The recording couldn't be read." |
| `not_scorable` | reject | "No landing was found to grade after the start." |
| `not_valid` | unverified | "A challenge rule wasn't met (see the checks)." |
| `marker_mismatch` | unverified | "The recording belongs to a different attempt." |
| `too_late` | unverified | "Uploaded after the deadline." |
| `outside_window` | unverified | "Flown outside the challenge's dates." |
| `duration` | unverified | "The flight took longer in the sim than in real time since the start (sim rate or pause)." |
| `duplicate` | unverified | "This recording was already used." |

Unknown reject codes and unverified reasons show their code with "See
details on <site>".

## A7. How an attempt is marked in the recording

The recorder gains one generic journal event, **code `8`, attempt
marker** ([format.md](format.md) event table), text a JSON object:

```json
{"phase":"begin","challenge":"ltfj-06-gusty-2026w44","version":1,"ref":"Qe3v1hJ0dEa9fP2kLm7xRw"}
{"phase":"end","challenge":"ltfj-06-gusty-2026w44","version":1,"ref":"Qe3v1hJ0dEa9fP2kLm7xRw","reason":"stopped"}
```

- `begin` is written when the aircraft is placed (teleport) or the
  attempt is armed (`self`); `end` when the attempt finishes or is
  abandoned (`reason`: `stopped`, `abandoned`, `timeout`, `replaced`).
- `ref` is an **opaque string** chosen by the caller
  (`^[A-Za-z0-9_-]{8,64}$`) or `null` for a local attempt. A site issues
  it per attempt (128 random bits); it is not a secret, it only ties the
  bytes to the issued attempt. No site origin, account or token is ever
  written into a flight.
- Older readers skip the unknown code (format rule). `fdm challenge`
  grades the touchdown after the **latest** `begin` marker when markers
  are present, and reports the marker it used
  (`"marker": {"ref": …, "begin_s": …, "end_s": …}`), so the server can
  compare `ref` with the attempt's.

## A8. Privacy

- Uploaded per attempt: the slice (A5), the client result (A4), app
  version, `client_attempt_id`. Nothing else: no other flight, no flight
  list, no file paths, no machine or Windows user name, no aircraft
  title or tail number, no telemetry.
- Nothing is uploaded without a signed-in site and an attempt the site
  issued; auto-upload is a per-site setting (default on, B3) and every
  upload is in the attempt log.
- The site learns the account from the token; the app never sends an
  email address.

## A9. Offline

- Offline or signed out **at start**: no attempt can be issued, so it is
  flown as **practice**: graded locally, **never uploaded**, not even
  later. The pre-flight card says so before placing (B8).
- Issued, network lost **later**: the attempt is flown, graded and sliced
  locally; the upload is queued and retried until `upload_by` (24 h
  after issue by default). The queue survives restarts (B5 sidecar).
- Signed out or site removed with queued uploads: they wait; on sign-in
  as the **same** user (`/me` id) they resume; as another user they are
  not sent (the attempt belongs to the first) and become
  `upload_abandoned`. "Sign out" warns first (B3).

## A10. What a server must do

1. Serve the well-known document (A1) and the API (A4) over https.
2. Implement A2: loopback redirect with PKCE S256 and exact `127.0.0.1`
   redirect URIs; device flow; short access tokens; rotated, revocable
   refresh tokens stored hashed, with the 60 s grace rule.
3. Bind each attempt to the account, the challenge **version** and the
   definition bytes (`definition_sha256`); issue idempotently per
   (`user`, `client_attempt_id`); one `ref` per attempt; `upload_by`.
4. Validate its published definitions with the same pinned `fdm`
   (`fdm challenge --check <definition.json>`), and re-score every upload
   with `fdm challenge <definition.json> <slice.fqz>` in isolation (no
   network, no secrets, time and memory limits): it parses untrusted
   binary input. Never rank on the client's result.
5. Check the marker `ref` against the attempt's, the sim duration against
   wall-clock time since issue, and uniqueness of the recording.
6. Never expose emails through the API or leaderboards (handles only).
7. Return `X-Request-Id`; log every state transition of every attempt
   with the request id and `definition_sha256` at issue, and every grant
   event (created, refreshed, grace re-issue, revoked with reason).
8. Let users see and delete their attempts and revoke app access on the
   site.

## A11. Versioning, compatibility and fixtures

- The protocol version is the integer in the well-known `versions` and in
  `X-Foqa-Protocol`. Within v1, changes are **additive only**: servers may
  add response fields, statuses, codes and optional endpoints; clients
  ignore unknown response fields, treat unknown statuses as not final
  (A4), unknown 4xx as "no retry" and unknown 5xx as "retry"; clients may
  add optional request fields, which servers ignore.
- Removing or changing the meaning of a field, an endpoint or a status is
  v2. A site keeps serving v1 for at least 6 months after FOQA-Sim ships
  v2 support (both listed in `versions`).
- The **challenge definition** and the **`Attempt` output** are versioned
  separately, by `crates/challenge` (`Challenge` fields, `scoring.version`).
  A site serves only definitions its pinned `fdm` accepts (`--check`); a
  client refuses definitions it cannot parse.
- **Fixtures:** `fixtures/protocol/` holds one JSON example per type and
  case, for the example site `https://landings.example`: the well-known
  document, token responses (one with `handle: null`), `/me`, a device
  code, `ChallengeEntry` lists, the issue response, an `AttemptStatus` per
  status (plus scored-unverified and an unknown status), a `/me/attempts`
  page and each A6 error body as `errors/<http>-<code>.json`. Definitions
  are embedded byte for byte under their `definition_sha256`.
  `crates/connect/tests/protocol_fixtures.rs` reads every one with the
  client's types; a site copies them into its tests and validates its own
  schemas against them. `fixtures/challenge/` holds a golden `challenge.json`, the
  synthetic `flight.fqz` it was flown in, that flight's `slice.fqz` and the
  expected `fdm challenge` output `attempt.json`, for a site's scorer CI:
  `fdm slice flight.fqz --ref Qe3v1hJ0dEa9fP2kLm7xRw` gives `slice.fqz`
  byte for byte, and `fdm challenge challenge.json slice.fqz` gives
  `attempt.json` (compare numbers to 1e-9 relative: another platform's
  maths may differ in the last bit). `crates/cli/tests/golden.rs` checks
  both; `FOQA_BLESS=1` rewrites them.

---

# Part B: the app side

## B1. Layout and boundary

```
crates/challenge   (core) definition, start_state, evaluate; + Mark (A7, opaque ref)
crates/recorder    (core) + attempt marker event, Recording::slice
crates/cli         (core) + `fdm slice`, `fdm challenge --check`
crates/connect     (NEW, optional) protocol client: discovery, auth,
                   API, deep-link parser, token store, upload queue,
                   the site sidecar (B5) and connect log
app/src-tauri
  src/attempts.rs  (core app) local attempt lifecycle + log (B5); knows
                   no origin, token or upload state
  src/connect.rs   (#[cfg(feature = "connect")]) Tauri commands,
                   deep-link handler, background uploader
app/src/pages/challenges/  Challenges screen; site parts render only
                   when the backend reports `connect` available
msfs-panel/        toolbar panel strip + EFB Challenges tab (B9)
```

- `crates/connect` depends on `challenge` and `recorder`; nothing depends
  on it except the app's optional feature
  `connect = ["dep:connect", "dep:tauri-plugin-deep-link", "dep:keyring"]`
  (default on). CI builds the app with `--no-default-features` and checks
  `cargo tree -i connect` lists only the app.
- **Without `connect`**, the Challenges screen still works: "Open
  challenge file…", briefing with pre-flight card, Place, the live
  attempt, Fly again, local grade, history and logs, the in-sim strip
  and EFB tab with local states. The marker and slice features are
  generic (a slice is also a way to share one landing).
- HTTP: `ureq` 3 (blocking, rustls with ring, webpki roots), 10 s
  connect / 60 s request timeouts, redirects off (a redirect to another
  origin would leak the token). `reqwest` is in the lock only through
  Tauri, without a TLS backend or its blocking client; enabling those
  pulls aws-lc-rs (CMake to build) and a Tokio runtime per client, while
  every call here is blocking on a worker thread anyway.
- Commands (added to [debrief-api.md](debrief-api.md) when built). Core:
  `challenge_open_file(path)`, `challenge_preflight(key)`,
  `attempt_create(key)`, `attempt_place(id)`, `attempt_finish(id)`,
  `attempt_abandon(id)`, `attempt_fly_again(id)`, `attempts_list`,
  `attempt_log(id)`, `attempt_delete(id)`. Connect: `connect_available`,
  `sites_list`, `site_add(origin)`, `site_sign_in(origin)`,
  `site_sign_out(origin)`, `site_remove(origin, delete_attempts)`,
  `site_challenges(origin)`, `attempt_upload(id)`, `deep_link_pending`.
  Events: `attempt-changed`, `deep-link` (`{site, id, v}`),
  `site-changed`.

## B2. Deep links in the app

- `tauri-plugin-deep-link` v2 with the static scheme in
  `tauri.conf.json` (`plugins.deep-link.desktop.schemes: ["foqa-sim"]`);
  the NSIS installer registers it; the app also calls
  `deep_link().register("foqa-sim")` at start-up (a reported NSIS
  registration issue), verified on a real NSIS install.
- **Forwarded** (app running): `tauri-plugin-single-instance`, still
  registered first, hands the second process's argv to its callback,
  which passes each `foqa-sim:` argument on as it is.
- **Cold start** (app not running): Windows starts the app with the URL
  as argv; the app reads `std::env::args()` at setup.
- Both read the **raw argument**, not the deep-link plugin's `Url`
  (`on_open_url`, `get_current`), which is already parsed: tabs and
  newlines stripped, non-ASCII percent-encoded, so A3's character checks
  would never see them. The plugin only registers the scheme.
- Both paths run A3's parser in `crates/connect` (pure, unit-tested with
  hostile inputs) and **buffer** the result until the frontend reports
  ready (`deep_link_pending`), so a link is not lost while the window
  loads. With the feature off or links disabled, nothing is registered or
  the URL is dropped.

## B3. Settings (Challenges → Sites)

Per site: name and origin (ASCII host), signed-in handle, "Upload
attempts automatically" (default on), "Sign out" (revoke + delete the
credential), "Remove site" (sign out, forget the site, optionally delete
its attempt folders). **Sign out / Remove warns** when uploads are
pending: "2 attempts haven't uploaded yet. They'll upload when you sign
back in as ege; signing in as someone else discards them." Global: "Open
challenge links from websites" (on/off), "Notify when a site scores an
attempt" (on), developer "Allow local sites" (off). Removing every site
leaves no network activity at all.

The two global switches are under the Sites list and kept in the app's
config folder as `sites-settings.json` (`{ "links": true, "notify": true }`;
a missing or unreadable file is the defaults). With links off a link is
logged (`deep_link`, reason `links_off`) and dropped without raising the
window; with notifications off no B7 toast is shown.

## B4. Token storage

**OS credential store via `keyring` 3.x: Windows Credential Manager**
(`windows-native`; the macOS Keychain and the Linux Secret Service with
`apple-native` and `sync-secret-service`), service `foqa-sim`, account
`<origin>`, value the refresh token only. The access token lives in
memory. A platform without a store that keeps the secret until deleted
counts as unavailable (below), never as `keyring`'s forgetful mock.

| | Credential Manager (keyring) | File in app data |
|---|---|---|
| At rest | DPAPI-encrypted per Windows user | Plain text unless we add DPAPI ourselves |
| Leaks via backups/sync/support zips | No | Yes (and `Documents` is often OneDrive) |
| Code | One crate, cross-platform API | Trivial |
| Survives uninstall | Yes: cleared on sign-out / Remove site | Deleted with app data |

If the store is unavailable, the app keeps the refresh token in memory
for the session and says it will ask again next start; it never falls
back to a file. The site list (origins, names, cached well-known,
settings; no secrets) is `%APPDATA%\com.egebilge.foqa-sim\connect\sites.json`.

## B5. Attempt lifecycle, files and logs

Each attempt has a folder `Documents\FOQA-Sim\challenges\<local id>\`
(local id: UUID v4, also sent as `client_attempt_id`). `Documents` may be
synced (OneDrive), so **nothing secret is ever written there**: no
token, code or verifier; the `ref` is not secret.

| File | Owner | |
|---|---|---|
| `attempt.json` | core | Challenge key/id/version, `ref` (opaque, or null), flight id, marker times, core state |
| `challenge.json` | core | The exact definition bytes flown |
| `log.jsonl` | core | Append-only core history (below) |
| `slice.fqz` | core | The slice (A5) |
| `result.local.json` | core | `evaluate` on the slice |
| `site.json` | connect | Sidecar: origin, server attempt id, `definition_sha256`, local deadlines, site state, last `AttemptStatus` |
| `site.log.jsonl` | connect | Append-only site history; the History view merges both logs by `at` |

**Core states** (`attempts.rs`, one owner thread, every transition
logged); the core ends at `done` and knows nothing of sites:

```
created ─▶ placing ─(place failed)─▶ failed_start
              │
              ▼
           flying ── gate ──▶ approach ── touchdown ──▶ rollout
              │                                          │
(cancel / Fly again / flight closed / 30 min)            ▼ (taxi speed on runway,
              ▼                                       finished   or 120 s, or "Finish")
           abandoned                                     │
                                          graded_local ─▶ sliced ─▶ done
```

- Order with a site: `attempt_create` (core, `created`) → connect issues
  with the local id (idempotent, so a lost response is re-asked with the
  same id) → `attempt_place(id, ref)`. Without a site, or offline:
  `attempt_place(id, None)` (practice).
- `gate`, `touchdown` and `rollout` are detected from the recording:
  after the `landing` notice ([debrief-api.md](debrief-api.md)), every
  5 s the app slices `[begin, now]` and runs `evaluate`; `finished` when
  the rollout reached taxi speed, 120 s passed, or the pilot pressed
  Finish.
- Placement writes `begin` (A7) through the recorder's request handle
  (`SimRequests`, the same path as `place`); `finished` or `abandoned`
  writes `end`.
- **Crash recovery** at start-up: an attempt left in `placing` … `rollout`
  is resolved from its recovered flight: a touchdown after the `begin`
  marker → `finished` (reason `recovered`) and graded; otherwise
  `abandoned` (reason `interrupted`). Connect resumes `issuing` by
  re-issuing with the same id.

**Site states** (`site.json`, connect only): `issuing` → `issued` →
`upload_pending` ⇄ `upload_retry` → `uploaded` → (server statuses, A6)
`queued`/`scoring`/`scored`/`rejected`/`failed`/`expired`/`removed`/
unknown; side exits `upload_refused`, `upload_abandoned`, `abandoned`;
`practice` when issue was impossible (never uploaded).

`log.jsonl` (core) and `site.log.jsonl` (connect), one object per line,
never rewritten:

```json
{"at":"2026-10-27T18:02:11.204Z","seq":1,"event":"created","challenge":"ltfj-06-gusty-2026w44","version":1,"definition_sha256":"…","app":"0.1.3"}
{"at":"…","seq":1,"event":"issued","origin":"https://pilotaptitude.example","attempt_id":"…","ref":"Qe3v…","definition_sha256":"…","definition_match":true,"fly_within_s":1800,"upload_within_s":86400,"request_id":"…","ms":182}
{"at":"…","seq":2,"event":"placed","flight_id":"flight-1793…","ref":"Qe3v…","start":{"lat_deg":…,"alt_ft":…},"aircraft":"B38M (transport)","preflight":{"aircraft":"ok","weather":"ok"}}
{"at":"…","seq":3,"event":"gate","sim_s":1834.2,"height_ft":954}
{"at":"…","seq":4,"event":"touchdown","sim_s":1902.8,"sink_fpm":142,"peak_g":1.18}
{"at":"…","seq":5,"event":"finished","reason":"stopped","sim_s":1941.0}
{"at":"…","seq":6,"event":"graded_local","valid":true,"score":87.4,"failed_checks":[]}
{"at":"…","seq":7,"event":"sliced","bytes":412733,"sha256":"…","from_s":1824.0,"to_s":1946.0}
{"at":"…","seq":2,"event":"upload","try":1,"http":503,"error":"busy","request_id":"…","ms":96,"retry_in_s":30}
{"at":"…","seq":3,"event":"upload","try":2,"http":202,"status":"queued","request_id":"…","ms":840}
{"at":"…","seq":4,"event":"server_status","status":"scored","score":87.4,"verified":true,"rank":3,"request_id":"…"}
```

**Connect log** `%APPDATA%\com.egebilge.foqa-sim\connect\connect.log.jsonl`
(not in `Documents`): `deep_link` (kind, origin, id, accepted or the
parser's reject reason), `consent` (connect prompt allow/deny), `sign_in`
(flow, outcome, request id), `refresh` (ok/http/error, grace), `revoke`,
`site_added`/`site_removed`. Rotated at 1 MiB, 3 files kept. Attempt logs
live and die with their folder (History → Delete).

Never logged anywhere: tokens, codes, verifiers, `state`, response bodies
beyond the fields above. Polls are logged only when the status changes.

## B6. Uploads, retries and stale attempts

A background uploader (connect) scans `site.json` sidecars in
`upload_pending`/`upload_retry` at start-up, on sign-in, on network
return and after each attempt. Backoff 5 s, 30 s, 2 min, 10 min, then
hourly, until the local upload deadline; `Retry-After` wins;
`checksum_mismatch` re-reads the slice and retries at most 3 times. Order
per attempt: recording, then client result, then polling. One upload at
a time.

Stale attempts: "Fly again" abandons the current site attempt
(`POST /attempts/{id}/abandon`, queued if offline) before issuing the
next; at start-up and before every issue, the app abandons its own
`issued` attempts that were never placed or whose fly window passed
without a flight, so the site's open-attempt cap is not hit by the app.

## B7. Notifications

Windows toasts (`tauri-plugin-notification`, already in the app) when
a site attempt becomes `scored` ("Scored 87.4, #3 on LTFJ 06 gusty"),
`rejected` or scored but unverified ("Scored 81.0, not ranked: flown
outside the wind band"), using the A6 copy. Only while the main window is
not focused; clicking opens the attempt. Setting in B3.

## B8. Briefing and pre-flight check (app, panel, EFB)

The briefing (start, gate, wind band, rules, source) carries a
**pre-flight card**, measured live by the app every 2 s while shown, each
line `ok` / `warn` / `fail` / `unknown` with a one-line fix:

| Check | Measured | Example copy (not ok) |
|---|---|---|
| Simulator | recorder status | "MSFS isn't connected. Start a flight in MSFS 2024." (fail: Place disabled) |
| Aircraft | `ATC MODEL` / class vs `aircraft` | "A C172 is loaded; this challenge needs a B38M or A20N. Load one, or fly as practice." |
| Weather | ambient wind now, resolved on the challenge runway, vs the bands | "Weather outside this challenge's wind band (crosswind 8 kt, needs 12 to 20). Change weather, or fly as practice." |
| Ranking | site, sign-in, last `/me` ≤ 60 s | "Offline: this attempt will be graded here but won't appear on the leaderboard." / "Local challenge file: graded here only." |

- Weather is `unknown` ("Checked after placing") when the aircraft is
  more than 30 nm from the airport with live weather; the strip then
  re-checks 10 s after placing and warns if outside the band.
- Buttons: **Place** (ranked when every line is ok or warn and the site
  is reachable) and **Fly as practice** (local only, never uploaded).
- The same card, from the same code, is sent to the panel and EFB (B9).

## B9. In-sim views: status strip, Fly again, EFB Challenges tab

Built on the panel feed ([ingame-panel.md](ingame-panel.md): `v: 2`,
6 KB cap, every app → view message repeated); added to that document
when built. The strip text is rendered by the app, so the views stay
dumb and the copy lives in one place.

App → view:

| Event | Payload | Cadence |
|---|---|---|
| `FOQA_SIM_ATTEMPT` | `AttemptStrip` | 1 Hz while an attempt is open and for 10 min after it reached a final state; at once on change |
| `FOQA_SIM_CHALLENGES` | `{ v, req, challenges: ChallengeCard[] }` (≤ 12, newest first) | reply to `list_challenges`, every 1 s for ~5 s |
| `FOQA_SIM_BRIEFING` | `{ v, req, key, title, lines: string[], preflight: PreflightLine[] }` | reply to `briefing`, every 1 s for ~5 s; the view re-asks every 5 s while shown |

```ts
interface AttemptStrip {
  v: 2;
  id: string;                  // local id
  key: string;                 // challenge key, for Fly again
  n: number;                   // attempt number on this challenge on this PC
  stage: "placing" | "flying" | "gate" | "rollout" | "finished" | "graded" | "abandoned" | "failed_start";
  landed?: { sink_fpm: number; peak_g: number };
  local?: { valid: boolean; score: number | null };
  site?: { state: "practice" | "uploading" | "waiting" | "scoring" | "scored" | "rejected" | "unverified"
                 | "expired" | "signed_out" | "unknown"; score?: number; rank?: number | null; reason?: string };
  text: string;                // "Attempt 2 · Gate · Landed 142 fpm · Uploading… · Scored 87.4, #3"
  can_fly_again: boolean;
}
interface ChallengeCard { key: string; title: string; airport: string; runway: string;
  source: string;              // "File" or "<name> (<ascii host>)"
  state: "open" | "upcoming" | "closed" | "needs_update"; last?: string; rank?: number | null }
interface PreflightLine { name: "sim" | "aircraft" | "weather" | "ranking";
  state: "ok" | "warn" | "fail" | "unknown"; text: string; fix?: string }
```

View → app (`FOQA_SIM_CMD`, acked in `Status.ack` like the others):

| Payload | |
|---|---|
| `{ v: 2, cmd: "list_challenges", req }` | answered in `FOQA_SIM_CHALLENGES` (cached list; no network call from the sim) |
| `{ v: 2, cmd: "briefing", req, key }` | answered in `FOQA_SIM_BRIEFING` |
| `{ v: 2, cmd: "place", key, practice?: boolean }` | create, issue (unless practice or offline) and place, as the app's Place |
| `{ v: 2, cmd: "fly_again" }` | abandon the current or last attempt (`end` reason `replaced`, site abandon), issue a new one on the same challenge and place |
| `{ v: 2, cmd: "finish" }` / `{ v: 2, cmd: "abandon" }` | as the app's buttons |

- **Toolbar panel:** the strip under the live board, with **Fly again**
  and **Finish** while an attempt is open.
- **EFB "Challenges" tab:** list (cards), briefing with the pre-flight
  card, Place / Fly as practice, last attempt and rank, Fly again.
- Local states arrive with F2; `site` fields with F4.

## B10. Implementation slices (FOQA-Sim; Ege commits)

| # | Slice | Network |
|---|---|---|
| F1 | Core: attempt marker event with opaque `ref` (journal, compact, readers, format.md), `Recording::slice` + `fdm slice` (A5 drops incl. `title`), `fdm challenge` grades after the latest marker and reports it, `fdm challenge --check`, `Attempt` gains `aircraft {icao, class}`, ~32 MiB decode cap; **golden `challenge.json` + `flight.fqz` + expected `Attempt` JSON** for site CI; protocol example fixtures (A11) | none |
| F2 | App, local: Challenges screen (open file, briefing, pre-flight card, Place / practice, live state, Finish, Fly again, local grade, history, delete), `attempts.rs` state machine, log and crash recovery; panel strip + EFB Challenges tab with local states (B9 messages) | none |
| F3 | `crates/connect`: discovery (ASCII host, IP-literal refusal), A3 parser, A2 auth (loopback with state on `error=` and Host check, PKCE, device fallback, single-flight refresh), API client, keyring store, sidecar + connect log; mock-server tests on the A11 fixtures | client only |
| F4 | App `connect` feature: Sites settings (sign-out warning), site challenges, issue (idempotent) → place → upload → poll, uploader queue, stale-attempt abandon, A6 copy, site states in strip/EFB, toasts, "View on site" | yes |
| F5 | Deep links (needs F4): plugin, single-instance `deep-link`, cold-start `get_current`, ready buffer, auto-resume to the briefing after connect, NSIS check, links toggle | — |
| F6 | `scripts/build-fdm-musl.sh`: reproducible static `fdm` + sha256 + VERSION for sites | — |

Each slice keeps the app fully usable with no site configured.

## Decisions (Ege, 2026-10-27)

- Upload window: `upload_by` = issue + 24 h; `fly_by` = issue + 30 min.
- Attempts without a site-issued attempt (offline at start, challenge
  files) stay local practice and are never uploaded.
- This protocol is published inside the FOQA-Sim repository, which goes
  public; sites link to its GitHub Releases (`releases/latest`) for
  downloads once public.
- No separate upload nonce: uploads are addressed by attempt id and
  authorised by Bearer + ownership, so the only per-attempt value the
  client keeps is the non-secret marker `ref` (confirmed by Ege,
  2026-10-27).
- Weather must be identical for every attempt (Ege, 2026-10-27): a
  ranked attempt starts with the challenge's exact wind, visibility, QNH
  and temperature set by the app, every time; measuring live weather and
  accepting a band is not enough. How the app sets it is under research
  (docs/challenges.md, "What MSFS 2024 allows").

## Implementation notes (F1, F3, F4, F5)

- `crates/connect` depends on no FOQA-Sim crate; the app's `connect`
  feature (default on) adds it with `open` (system browser). Off
  (`cargo build -p foqa-sim-app --no-default-features`), the app has no
  network code for challenges; `cargo tree -i connect` lists only the app.
- The core joins it through `commands::AttemptHooks` (issue before
  placing, the pre-flight ranking line) and the attempt change callback
  (done → upload, abandoned / failed start → site abandon). The core's
  states and files are unchanged; `site.json` and `site.log.jsonl` sit in
  the attempt folder (the app keeps attempt folders in its data folder,
  not `Documents`).
- Sites come only from the user: Challenges → Sites → Add (deep links are
  F5). A site's challenge is taken into the library as its exact bytes
  (checked against `definition_sha256` and by `Challenge::parse`); an
  attempt at it is issued under the local id, and a definition other
  than the one issued makes it practice.
- **Marker (F1):** `SimEvent::Attempt` (journal code 8, format.md),
  written through `SimRequests::attempt_marker` into the open flight only.
  `AttemptHooks::before_place` returns the issued `ref` (connect: the
  sidecar's, when issued and the definition matched), kept in
  `attempt.json` as `ref`; `begin` follows the placement (or the arming of
  a `self` start), `end` precedes the grade (`stopped`) or an abandon
  (`abandoned`, `replaced`) while that flight still records.
- **Slice (F1):** an attempt with a `ref` goes `graded_local` → `sliced` →
  `done`: `recorder::slice` cuts `slice.fqz` from the flight (journal or
  compact) on a thread of its own, waiting up to 15 s for the recording to
  pass 5 s after the end marker; `sliced` logs bytes, SHA-256 and window.
  A failed cut is logged and retried at the next start. A local attempt
  (`ref` null) goes `graded_local` → `done`, with no slice.
- **Upload:** only `slice.fqz`; an attempt without one is refused
  (`upload_refused`, `no_slice`), never replaced by the whole flight.
- **`fdm challenge`** grades only a touchdown after the latest `begin`
  marker and reports `marker` (`ref`, `begin_s`, `end_s`) and `aircraft`
  (`icao`, `class`) in the `Attempt`.
- **`Attempt.assessment` (additive, 2026-10-28):** a `landing-2` attempt
  carries `assessment: { method, score, grade, confidence, caps }` from
  `crates/assess`, with the debrief wire's types (`Grade`: `good`, `fair`,
  `poor`; `Confidence`: `full`, `partial`; `caps`: `CapNote`
  `{ kind, score, reason, source }`, `kind` one of `material`, `unstable`,
  `inspection`, `undershoot`, `excursion`). `score` equals `score.total`
  and `caps[].score` is the limit the cap sets. Absent for `landing-1`
  and for an attempt without a score. A site no longer derives grade,
  confidence or caps from `score.parts`.
- **Decode cap:** `fdm challenge` and `fdm slice` refuse a compact
  flight whose body decompresses past 32 MiB
  (`recorder::CHALLENGE_MAX_BODY`); the app's own flights keep 256 MiB.
- **Not in F1 yet:** the live `[begin, now]` grading during the rollout
  (B5).
- "Allow local sites" is the environment variable
  `FOQA_ALLOW_LOCAL_SITES=1` until the app has a developer settings
  screen; the links and notifications switches (B3) are built, the
  per-site auto-upload toggle and the stale-attempt sweep at start-up (B6)
  are not yet.
- **Deep links (F5):** `crates/connect/src/deeplink.rs` parses A3 strictly
  (512 bytes, printable ASCII only, host `challenge`/`connect` exactly, no
  userinfo/port/path/fragment, a repeated key refused, `site` by A1's
  `Origin::parse`, `id` and `v` as A3) and holds the one pending link
  and the one waiting behind it (`Gate`, 10 minutes, memory only), each
  with a `seq` that every answer names (`deep_link_consent`,
  `deep_link_open`, `deep_link_done`): a click acts only on the link it
  was shown for. An unknown site is `ask` until `deep_link_consent`; a
  known one is `open`, and `deep_link_open` (the Open click) is the only
  path that fetches and keeps its challenge. The `deep-link` event carries
  nothing: the window asks `deep_link_pending` while it shows no link,
  which gives a refused link's words once (apart from the links waiting,
  so it drops none) or the link waiting, once. The app registers
  `tauri-plugin-single-instance` first, then `tauri-plugin-deep-link`
  (static scheme in `tauri.conf.json`) and calls `register("foqa-sim")`
  at start-up; links come from argv (B2). Every link is logged in the
  connect log (`deep_link`: kind, origin, id, v, `duplicate`, or the
  reject reason and length, never the URL), every answer too (`consent`).
  The window then asks as A3 says. Without `connect` the scheme is still
  registered by the installer; the app recognises `foqa-sim:` in its
  arguments (first start or second instance) and only says "Sites are
  disabled in this build".
- **Toasts (B7):** on a sidecar's change to `scored` or `rejected`, once
  per attempt, only while the window isn't in front and "Notify when a
  site scores an attempt" is on: "Scored 87.4, #3
  on <title>" with the site, "Scored 81.0, not ranked on <title>" with the
  reasons, "Rejected on <title>" with the reject copy; a click opens the
  challenge (`open-challenge`).
- **In-sim views (B9):** `crates/capture/src/views.rs` (types, size caps,
  `AppCommand`, the app's `Views` handle) and `app/src-tauri/src/insim.rs`
  (the strip's words, the cards, the briefing lines). The strip follows the
  open attempt, else the newest, rebuilt on every attempt change and every
  5 s; a final one (core done and the site's state final, or no site) goes
  on for 10 minutes. While it flies, the app grades the open flight every
  5 s for the gate, which moves the attempt to `approach`
  (logged `gate` once). `AttemptStrip` also has `can_finish`. Views:
  `foqa-strip.js` (toolbar board and EFB), `efb/foqa-challenges.js`.
