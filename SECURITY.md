# Security Policy

## Known Vulnerabilities

### High Severity

#### 1. rustls-webpki v0.103.13 — DoS via malformed CRL
- **CVE/GHSA**: tracked through our dependencies
- **Affected surface**: TLS certificate verification on connection
- **Status**: pinned by the rustls 0.23.x dependency chain
- **Plan**: wait for a stable rustls-webpki v0.104 release
- **Mitigations**:
  - Use only on trusted networks
  - Consider client certificate pinning

#### 2. russh v0.59.0 — pre-auth unbounded allocation
- **CVE/GHSA**: tracked through our dependencies
- **Affected surface**: keyboard-interactive SSH authentication
- **Status**: upgrading to the 0.60.x line involves breaking changes
- **Plan**: impact is limited as long as keyboard-interactive auth is not used
- **Mitigations**:
  - Use password or public-key authentication only
  - Disable keyboard-interactive authentication

### Low Severity

#### lru crate — Stacked Borrows violation
- **Affected surface**: only when using the internal iterator
- **Assessment**: safe in practice for our usage

## Dependency Monitoring

Dependabot alerts are visible here:
https://github.com/mizu-jun/Nexterm/security/dependabot

## Update Policy

| Severity | SLA | Action |
|----------|-----|--------|
| Critical | Immediate | Emergency patch or fork |
| High | Within 30 days | Track upstream / consider forking |
| Medium | Within 90 days | Regular update cycle |
| Low | Next release | Accept or track |

## Reporting Vulnerabilities

Please report vulnerabilities via GitHub Security Advisories.
