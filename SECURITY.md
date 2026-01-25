# Security Policy

## Supported Versions

We actively support the following versions of PJS with security updates:

| Version | Supported          | Notes                                    |
| ------- | ------------------ | ---------------------------------------- |
| 0.4.x   | :white_check_mark: | Current stable release (recommended)     |
| 0.3.x   | :x:                | Unsupported - upgrade to 0.4.x           |
| < 0.3   | :x:                | Unsupported - upgrade to 0.4.x           |

**Recommendation:** Always use the latest patch version within the supported major.minor release.

### Version Support Policy

- **Current Release**: Receives all security updates, bug fixes, and feature updates
- **Previous Patch Releases**: Security fixes backported for critical vulnerabilities (CVSS >= 7.0)
- **EOL Versions**: No security support - users must upgrade

**Minimum Supported Rust Version (MSRV):** Rust nightly 1.89+

## Reporting a Vulnerability

We take security seriously. If you discover a security vulnerability in PJS, please report it responsibly.

### Reporting Process

**DO NOT** create a public GitHub issue for security vulnerabilities.

**Preferred Method: GitHub Security Advisories**

1. Go to https://github.com/bug-ops/pjs/security/advisories
2. Click "Report a vulnerability"
3. Fill out the advisory form with:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)
   - Affected versions

**What to Include:**

1. **Description**: Clear explanation of the vulnerability
2. **Reproduction**: Step-by-step instructions to reproduce
3. **Impact**: Potential security impact (confidentiality, integrity, availability)
4. **Affected Versions**: Which versions are vulnerable
5. **Suggested Fix**: Proposed remediation (optional)
6. **Proof of Concept**: Code or steps demonstrating the issue (if applicable)
7. **Your Information**: Name and contact details (for credit and follow-up)

### Response Timeline

We are committed to responding promptly:

| Timeframe | Action |
|-----------|--------|
| **24 hours** | Initial acknowledgment of your report |
| **72 hours** | Preliminary assessment and triage (CVSS scoring) |
| **7 days** | Detailed response with timeline for fix |
| **30 days** | Security patch released (for critical vulnerabilities) |
| **60 days** | Public disclosure (coordinated with reporter) |

**Critical Vulnerabilities (CVSS >= 9.0):**
- Hotfix released within 7 days
- Immediate notification to known users
- Emergency security advisory published

**High Severity (CVSS 7.0-8.9):**
- Patch released within 30 days
- Included in next regular release or emergency patch

**Medium/Low Severity (CVSS < 7.0):**
- Fix included in next scheduled release
- Documented in CHANGELOG.md

### Coordinated Disclosure

We follow responsible disclosure practices:

1. **Private Discussion**: We work with you privately to understand and fix the issue
2. **Patch Development**: We develop and test a fix
3. **Security Advisory**: We prepare a security advisory draft
4. **Coordinated Release**: We coordinate public disclosure with you
5. **Credit**: We give you credit in the security advisory (unless you prefer to remain anonymous)

**Disclosure Timeline:**
- Default: 90 days from initial report
- Extended on request if fix is complex
- Accelerated for actively exploited vulnerabilities

## Security Update Notifications

Stay informed about security updates:

1. **GitHub Security Advisories**: https://github.com/bug-ops/pjs/security/advisories
2. **GitHub Watch**: Enable "Releases only" or "All activity" notifications
3. **Changelog**: Review `CHANGELOG.md` for security fixes
4. **Cargo Audit**: Run `cargo audit` regularly in your projects

**Subscribe to Updates:**
```bash
# Add PJS to your dependency audit
cargo install cargo-audit
cargo audit

# Check for advisories specifically for PJS
cargo audit --db cargo-advisory-db
```

## Scope

### In Scope

The following are **in scope** for security vulnerability reports:

**Core Library (`pjson-rs`):**
- Memory safety violations (buffer overflows, use-after-free)
- Denial of Service (DoS) attacks
  - Decompression bombs
  - Excessive memory allocation
  - Infinite loops or excessive CPU usage
  - Stack overflow from deeply nested JSON
- Input validation bypass
- Schema validation bypass
- Integer overflow/underflow
- Unsafe code vulnerabilities

**WebAssembly (`pjs-wasm`):**
- WASM memory safety issues
- JavaScript interop vulnerabilities
- WASM-specific DoS vectors
- Security limit bypass

**HTTP/WebSocket Servers:**
- Authentication/authorization bypass
- CORS misconfiguration exploitation
- Header injection
- Request smuggling
- WebSocket protocol violations

**Security Features:**
- Rate limiting bypass
- Compression bomb protection bypass
- Depth tracking bypass
- Size limit bypass

### Out of Scope

The following are **not considered security vulnerabilities**:

- Vulnerabilities in dependencies (report to the upstream project)
- Performance issues without security impact
- Feature requests or enhancements
- Issues in demo applications (`pjs-demo`, examples)
- Third-party integrations (unless PJS code is vulnerable)
- Social engineering attacks
- Physical security
- Denial of service requiring significant resources (e.g., 1000+ servers)

**Exceptions:**
- Dependency vulnerabilities affecting PJS will be tracked and addressed
- We monitor our dependencies with `cargo-audit` and will update vulnerable dependencies

## Security Best Practices for Users

### Recommended Configuration

**Production Deployment:**

```rust
use pjson_rs::infrastructure::http::axum_adapter::create_pjs_router;
use tower_http::limit::RequestBodyLimitLayer;

let app = create_pjs_router()
    .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024))  // 10 MB limit
    .with_state(app_state);
```

**WASM Security Limits:**

```javascript
import { PriorityStream, SecurityConfig } from 'pjs-wasm';

const security = new SecurityConfig()
    .setMaxJsonSize(5 * 1024 * 1024)  // 5 MB max
    .setMaxDepth(32)                   // 32 levels max
    .setMaxArraySize(10000)            // 10K elements max
    .setMaxObjectKeys(10000);          // 10K keys max

const stream = PriorityStream.withSecurityConfig(security);
```

### Security Checklist

- [ ] **Validate Input**: Always validate JSON size before processing
- [ ] **Set Limits**: Configure appropriate depth, size, and count limits
- [ ] **Use Latest Version**: Keep PJS updated to the latest patch version
- [ ] **Audit Dependencies**: Run `cargo audit` regularly
- [ ] **Rate Limiting**: Implement rate limiting for public endpoints
- [ ] **Error Handling**: Don't expose sensitive information in error messages
- [ ] **Monitor Resources**: Track memory and CPU usage
- [ ] **Timeouts**: Set appropriate timeouts for parsing and streaming

### Built-in Security Features

PJS includes defense-in-depth security:

**Decompression Protection (v0.4.7+):**
- MAX_RLE_COUNT: 100,000 items per run-length encoded segment
- MAX_DELTA_ARRAY_SIZE: 1,000,000 elements in delta-encoded arrays
- MAX_DECOMPRESSED_SIZE: 10 MB total output
- Integer overflow protection: Checked arithmetic throughout

**Parsing Limits:**
- Max JSON depth: 64 levels (configurable)
- Max JSON size: 10 MB (configurable)
- Max array elements: 10,000 (configurable)
- Max object keys: 10,000 (configurable)

**Rate Limiting:**
- Configurable per-session and global limits
- Automatic cleanup of expired sessions
- Bounded memory usage

## Security Advisories

### Published Advisories

**v0.4.7 (2026-01-25) - Decompression Vulnerabilities Fixed**

Fixed 3 critical vulnerabilities in decompression algorithms:

- **VULN-001**: RLE Decompression Bomb (CVSS 7.5 → 0.0)
  - Unlimited run-length encoding could exhaust memory
  - Fixed: MAX_RLE_COUNT limit (100,000 items)

- **VULN-002**: Delta Array Size Validation (CVSS 7.5 → 0.0)
  - Delta decompression could create unbounded arrays
  - Fixed: MAX_DELTA_ARRAY_SIZE limit (1,000,000 elements)

- **VULN-003**: Integer Overflow in Delta Decompression (CVSS 7.5 → 0.0)
  - Integer overflow in cumulative delta calculation
  - Fixed: Checked arithmetic with overflow protection

**Affected Versions:** 0.3.x - 0.4.6
**Fixed In:** 0.4.7
**Mitigation:** Upgrade to 0.4.7 or later

### Past Security Issues

We maintain a history of security issues in our changelog. See `CHANGELOG.md` for details.

## Security Scanning

### Automated Scanning

Our CI pipeline includes:

- **OSV Scanner**: Weekly vulnerability scanning on Saturdays at 17:39 UTC
- **Cargo Audit**: Dependency vulnerability checks
- **Clippy**: Static analysis with security lints enabled
- **Codecov**: Code coverage monitoring (87.35% coverage)

### Manual Security Reviews

We conduct periodic security reviews:

- Code review for all PRs
- Architecture review for major changes
- Dependency audit before releases
- Penetration testing for HTTP/WebSocket features (on request)

## Responsible Disclosure Hall of Fame

We appreciate security researchers who help us keep PJS secure. Contributors who responsibly disclose vulnerabilities will be acknowledged here (with permission).

**Contributors:**
- (None yet - be the first!)

## Additional Resources

- **Rust Security Database**: https://rustsec.org/
- **OWASP Top 10**: https://owasp.org/www-project-top-ten/
- **CWE (Common Weakness Enumeration)**: https://cwe.mitre.org/
- **CVE (Common Vulnerabilities and Exposures)**: https://cve.mitre.org/

## Questions

If you have questions about this security policy, please contact:

- Email: k05h31@gmail.com
- GitHub Discussions: https://github.com/bug-ops/pjs/discussions

---

**Thank you for helping keep PJS and its users secure!**
