# Security Policy

## Reporting a vulnerability

Please report security issues **privately** using GitHub's security advisories:

https://github.com/jere344/windows-dual-audio/security/advisories/new

Do not open a public issue for security problems.

## Scope

Dual Audio runs entirely on your machine. It captures audio through the system audio APIs (WASAPI on Windows, PulseAudio/PipeWire on Linux) and
plays it back to local output devices. It does **not** transmit audio or any data over the network,
and it collects no telemetry.
