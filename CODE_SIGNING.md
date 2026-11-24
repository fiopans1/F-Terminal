# Code signing policy

This project uses free code signing provided by
[SignPath.io](https://signpath.io/), with a certificate issued by
[SignPath Foundation](https://signpath.org/).

## Signed artifacts

Windows release artifacts are built from this repository on GitHub-hosted
runners and signed through SignPath.io:

- `f-terminal.exe`
- `F-Terminal-Setup-<version>-x64.exe`

The publisher shown by Windows for these binaries is **SignPath Foundation**.

The Inno Setup uninstaller generated at install time is not signed by the
SignPath flow. Builds that use a local Authenticode PFX sign it as well.

## Roles

F-Terminal is maintained by a single maintainer. The same person currently
holds every signing role:

| Role | Person |
|---|---|
| Author | [@fiopans1](https://github.com/fiopans1) |
| Reviewer | [@fiopans1](https://github.com/fiopans1) |
| Approver | [@fiopans1](https://github.com/fiopans1) |

Every change reaches the signing workflow through the repository's normal Git
history. Every SignPath signing request must be approved manually by the
approver. Multi-factor authentication is required for GitHub and SignPath.

## Build and signing rules

- Only binaries built from this repository's source are signed.
- Signing requests are created only by the GitHub Actions workflow from a
  release tag (`v*`) in this repository.
- The workflow runs on GitHub-hosted runners.
- Artifacts are uploaded to GitHub before they are submitted to SignPath, so
  SignPath can verify their origin.
- Signed files are not modified after signing. Checksums are generated after
  signing.

## Privacy

F-Terminal does not include telemetry. It makes network requests only when the
user invokes a feature that needs them (Git remotes, GitHub/GitLab API calls,
forge token validation, or opening external URLs). Tokens are read from the
local `config.toml` and are not sent anywhere except the configured forge API.

## Source

- Repository: <https://github.com/fiopans1/F-Terminal>
- License: AGPL-3.0-only
- Releases: <https://github.com/fiopans1/F-Terminal/releases>
