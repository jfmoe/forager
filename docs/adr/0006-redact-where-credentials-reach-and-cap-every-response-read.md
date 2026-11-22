# Redact where credentials can reach and cap every response read

## Decision

**Redaction scope.** Credential redaction guards the surfaces a credential can actually reach:
error messages, source URLs and titles, journal entries, diagnostics, and configuration and
doctor views. The answer body and fetched page content are exempt, and streamed answer deltas are
appended verbatim. Where ADR-0005 says no output contains a credential, it means these surfaces.

**Response caps.** Every HTTP response read is bounded by fixed limits in code, not by
configuration: one for successful bodies and streams, a smaller one for error bodies. Streams are
capped below the event parser, so a single oversized event cannot buffer without bound, and a cut
always lands on a character boundary.

**What a cap means.** Only Web Fetch may turn a capped prefix into success, with a truncation
diagnostic, and it falls through to the next provider only when the surviving prefix is thin.
Every other body is a protocol envelope (search streams, JSON APIs, MCP responses): reaching the
limit is a runtime failure, and forager never synthesizes a result from incomplete framing. An
error body is cut silently, because only its first part reaches the error message.

## Why

- A provider cannot know the caller's credentials, so generated answers and third-party pages
  cannot echo them. A provider that does echo a credential back does so in an error, and the error
  path stays redacted. Redacting every streamed delta guarded a surface no credential reaches.
- The caps are safety rails against memory exhaustion, not tuning knobs: no user is better placed
  than the code to choose them, so they are not configurable.
- A page prefix is still the page's own text and useful evidence. A truncated protocol envelope
  is not: completing it by guesswork would report a partial answer as a whole one.

## Rejected alternatives

- **Redacting all output, including answers and page bodies.** No credential can appear there, so
  the work protected nothing.
- **Configurable caps.** They would invite raising a limit to work around one response, which
  removes the protection for all of them.
- **Accepting a capped protocol response as partial success.** The caller could not tell a
  complete answer from a cut one.

## Revisit when

- A provider is found to reflect request credentials into answer text or page content.
- Legitimate protocol responses start reaching the success cap.
