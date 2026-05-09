# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.3.0 (2026-05-09)

### Chore

 - <csr-id-852314bb924b3bd6e78c407452e1cbfdebdd3d16/> bump version to 0.3.0

### New Features

 - <csr-id-36655538885e4feb1debd9a157f240f3c17acae7/> initial release of confit
   Config resolver with interpolation, shell evaluation, pluggable providers,
   secret masking, and SSH agent management. Includes CLI, wiki, CI/CD pipelines,
   and 85 tests.

### Bug Fixes

 - <csr-id-c448cf344f0ce902c442dd3f243de84466bfc8ae/> install script unbound variable and cargo fmt
   Move tmp dir to global scope so the EXIT trap can access it under
   set -u. Run cargo fmt on all files.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 5 commits contributed to the release.
 - 3 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Bump version to 0.3.0 ([`852314b`](https://github.com/krondor-corp/confit/commit/852314bb924b3bd6e78c407452e1cbfdebdd3d16))
    - Install script unbound variable and cargo fmt ([`c448cf3`](https://github.com/krondor-corp/confit/commit/c448cf344f0ce902c442dd3f243de84466bfc8ae))
    - Merge pull request #1 from krondor-corp/release-automation ([`cfa85af`](https://github.com/krondor-corp/confit/commit/cfa85af8ad448c78ca511be44d8b4747762cce92))
    - Bump confit-core v0.2.0, confit-cli v0.2.0 ([`233ae41`](https://github.com/krondor-corp/confit/commit/233ae4199b3727ee8a0a97c26a32223e1bd30d6e))
    - Initial release of confit ([`3665553`](https://github.com/krondor-corp/confit/commit/36655538885e4feb1debd9a157f240f3c17acae7))
</details>

## v0.2.0 (2026-05-09)

### New Features

 - <csr-id-36655538885e4feb1debd9a157f240f3c17acae7/> initial release of confit
   Config resolver with interpolation, shell evaluation, pluggable providers,
   secret masking, and SSH agent management. Includes CLI, wiki, CI/CD pipelines,
   and 85 tests.

