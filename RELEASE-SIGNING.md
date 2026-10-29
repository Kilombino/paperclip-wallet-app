# Paperclip release signatures

Paperclip uses one OpenPGP release identity across its repositories and packages:

- Identity: **Paperclip Holdings LLC (Release Signing)**
- Primary fingerprint: `AC121D21CE3BB408763DD608165128AF9E2F3216`
- Signing subkey: `C286AC4A28045C7BDFDB4EA88DB2EC047CA729C5`
- Current expiry: October 6, 2028
- Public key: [paperclip-release-signing-key.asc](paperclip-release-signing-key.asc)

This policy applies to Paperclip releases, including beta packages.
It does not authenticate upstream releases or imply that older downloads have
signatures. A release is GPG-signed only when its verified checksum manifest has
a valid signature from this identity.

## Verify a download

Download the package, `SHA256SUMS`, `SHA256SUMS.asc`, and
`paperclip-release-signing-key.asc` from the same release. Compare the full primary
fingerprint with a trusted, independent source before you trust a new key.

```sh
gpg --show-keys --with-fingerprint paperclip-release-signing-key.asc
gpg --import paperclip-release-signing-key.asc
gpg --verify SHA256SUMS.asc SHA256SUMS
# Continue only after a valid signature from the expected identity.
sha256sum --check SHA256SUMS
# macOS alternative: shasum -a 256 -c SHA256SUMS
```

The checksum for the package you install must match. Do not ignore failed or
missing checks. A signature authenticates the release bytes. It does not prove
that the software is audited or that a build is reproducible.

## Release requirements

Before publication, the release maintainer must review the build provenance and
verify the final package checksums. Sign the manifest with the existing Paperclip
key. Publish the manifest, detached signature, and public key beside the packages.
Include source revision and build information in the release notes. For container
releases, include a file with immutable image digests in the signed manifest.
Do not substitute a mutable image tag for a digest.

Keep private keys and passwords outside repositories and build logs. Do not create
a new GPG identity for each repository. This document establishes the release
policy; it does not itself add automated enforcement to build workflows.

StartOS native package signatures and Apple code signatures use separate formats
and keys. Preserve those keys and signatures. GPG signs the final downloadable
package checksums in addition to native signatures. Umbrel does not automatically
verify this detached GPG signature during installation.

Do not replace existing release binaries or rewrite historical tags to adopt this
policy. If a verified historical release receives a detached signature later,
state the date and retrospective nature in its release notes. Commit signatures
and GitHub's Verified badge are separate from release artifact signatures.

Renew this identity before expiry. Publish any revocation or replacement through
the established release channels, with a transition signed by the existing key
when it remains trustworthy.
