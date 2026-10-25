# Security Policy

## Reporting

Do not open a public issue for a vulnerability that could expose source code, credentials, or host data. Until a private security contact is configured on the eventual GitHub repository, retain the report locally and contact the repository owner through an agreed private channel.

## Current threat model

Linxts treats source files, serialized IR, cache entries, model responses, and provider errors as untrusted input.

Security invariants:

- invalid UTF-8 and oversized source files are rejected before parsing;
- parser, IR nesting, repair passes, loops, and function calls are bounded;
- model output must be decoded into versioned typed structures and pass IR validation;
- aggressive repair cannot invent external side effects;
- online source upload requires explicit authorization;
- credentials must come from environment variables or a future secure credential store and must never be logged;
- CI does not download models or require secrets.

The MVP VM has no filesystem, subprocess, networking, FFI, import, or dynamic-evaluation operations. Native code generation is not present.

## Supported versions

No stable version has been released. Security fixes currently target the latest `main` branch only.
