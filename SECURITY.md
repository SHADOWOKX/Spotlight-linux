# Security Policy

## Supported version

Spotlight Linux is currently in preview. Security fixes are applied to the latest preview release and the `main` branch.

## Reporting a vulnerability

Please do **not** open a public issue for a vulnerability that could put users at risk.

If GitHub's private vulnerability reporting option is available for this repository, use it. Otherwise, contact the maintainer through the GitHub profile before publishing technical details.

Please include:

- affected version or commit
- Linux distribution and desktop/session type
- clear reproduction steps
- security impact
- relevant logs or diagnostics with secrets and personal data removed

I will acknowledge valid reports as soon as practical and coordinate a fix before public disclosure when appropriate.

## Scope

Useful security reports include unsafe application launching, command injection, unsafe file permissions, symlink/hardlink handling, unintended data exposure, insecure update/install behavior, and privilege-boundary issues.

Normal feature bugs and crashes should use the regular bug report template.
