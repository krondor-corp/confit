# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.3.0 (2026-06-08)

### Chore

 - <csr-id-ae29dc00d1affb38a2317d17d6abcc779fb16205/> bump version to 0.3.0

### New Features

 - <csr-id-f974a2770e03ed1e1cc2cca2b1d996284a161733/> add [sources] for bulk secret/env loading with lazy caching
   Introduces a [sources] table alongside [providers]. A source runs its
   load command once, parses the dotenv output, and memoizes the result
   for the entire resolution pass — eliminating the N-subprocess problem
   where N keys all pointed at the same bulk-fetch tool (infisical, op
   environment read, etc.).
   
   Key behaviours:
   - String shorthand: sources.bag = "infisical export --env=prod --format=dotenv"
   - Table form: [sources.bag] with load/secret/format fields
   - secret = true marks every field from that source as secret
   - secret://source://FIELD composites also work
   - {vars.*} and bare {varname} both expand in load templates
   - {path}/{uri} are rejected in load templates (load is per-source, not per-key)
   - env://FOO is a built-in source backed by the process environment
   - Missing fields are hard errors, not empty strings
 - <csr-id-36655538885e4feb1debd9a157f240f3c17acae7/> initial release of confit
   Config resolver with interpolation, shell evaluation, pluggable providers,
   secret masking, and SSH agent management. Includes CLI, wiki, CI/CD pipelines,
   and 85 tests.

### Bug Fixes

 - <csr-id-8d2b7bbb7e6c49a470bde398b5afbc5d58906376/> fix clippy warnings (map_or, too_many_arguments)
 - <csr-id-4c7572b2d286cd91463ff04fa7ff68522caef908/> fix rustfmt formatting
 - <csr-id-c448cf344f0ce902c442dd3f243de84466bfc8ae/> install script unbound variable and cargo fmt
   Move tmp dir to global scope so the EXIT trap can access it under
   set -u. Run cargo fmt on all files.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 9 commits contributed to the release.
 - 6 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Bump version to 0.3.0 ([`ae29dc0`](https://github.com/krondor-corp/confit/commit/ae29dc00d1affb38a2317d17d6abcc779fb16205))
    - Merge pull request #9 from krondor-corp/feature/kro-160-add-cached-sources-for-bulk-secretenv-loading ([`bb359fb`](https://github.com/krondor-corp/confit/commit/bb359fb34d37e1abde70a4ffee58570a327e3e54))
    - Fix clippy warnings (map_or, too_many_arguments) ([`8d2b7bb`](https://github.com/krondor-corp/confit/commit/8d2b7bbb7e6c49a470bde398b5afbc5d58906376))
    - Fix rustfmt formatting ([`4c7572b`](https://github.com/krondor-corp/confit/commit/4c7572b2d286cd91463ff04fa7ff68522caef908))
    - Add [sources] for bulk secret/env loading with lazy caching ([`f974a27`](https://github.com/krondor-corp/confit/commit/f974a2770e03ed1e1cc2cca2b1d996284a161733))
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

