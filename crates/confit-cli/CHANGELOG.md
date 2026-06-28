# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.5.0 (2026-06-28)

### Chore

 - <csr-id-2f1c3d65f2e057aae5403787aa1ad7e29d33faf4/> bump version to 0.5.0
 - <csr-id-22005abbaa32cecf19bc60b3443f59ba290dfc4d/> bump version to 0.4.0
 - <csr-id-ae29dc00d1affb38a2317d17d6abcc779fb16205/> bump version to 0.3.0

### New Features

 - <csr-id-642d57c11bc2b0f791b74656d2f5bb103fad966c/> add `confit export` and `[env]` profiles for one-shot env materialization
   Materialize a complete, multi-section dev env once (one provider/1Password
   unlock) into a safe file an agent or fresh shell can source repeatedly.
   
   confit export [--profile <name> | <SECTION>...] [OPTIONS]
     - composes multiple sections (later wins on key conflict) or a named
       [env.<name>] profile drawing from sections/providers/sources/literals
     - --out writes 0600, atomically (temp+rename), and refuses non-gitignored
       paths (checked via `git check-ignore`) unless --force; warns outside a repo
     - --format dotenv|shell|json (all shell-safe single-quoted)
     - --reveal required to emit secret values (refuses rather than write masked
       junk); with --out, secrets go only to the file and a keys-only summary
       prints to stderr
     - fails closed: nonzero exit and no file written on any unresolved value
     - --upper / --prefix / --no-eval carried over from show
   
   [env.<name>] profiles compose a named env declared once in config. Profiles can
   pin their own vars via `vars.stage = "..."` (precedence: [vars] < profile vars
   < CONFIT_VAR_* < --set). Profiles and export share a single SourceCache, so a
   [sources] bag referenced across many keys loads only once.
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

 - <csr-id-c448cf344f0ce902c442dd3f243de84466bfc8ae/> install script unbound variable and cargo fmt
   Move tmp dir to global scope so the EXIT trap can access it under
   set -u. Run cargo fmt on all files.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 15 commits contributed to the release.
 - 7 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Bump version to 0.5.0 ([`2f1c3d6`](https://github.com/krondor-corp/confit/commit/2f1c3d65f2e057aae5403787aa1ad7e29d33faf4))
    - Merge pull request #13 from krondor-corp/release-automation ([`f31a175`](https://github.com/krondor-corp/confit/commit/f31a175709965b9ddd112d042d4c193c04ed77f9))
    - Bump confit-core v0.4.0, confit-cli v0.4.0 ([`2e05a27`](https://github.com/krondor-corp/confit/commit/2e05a27832c0a850b9004af57cc81f81d755c13c))
    - Bump version to 0.4.0 ([`22005ab`](https://github.com/krondor-corp/confit/commit/22005abbaa32cecf19bc60b3443f59ba290dfc4d))
    - Merge pull request #12 from krondor-corp/feature/materialized-env ([`0568f3f`](https://github.com/krondor-corp/confit/commit/0568f3fa1ff2cc95dae53a84e805ccc6947cffa2))
    - Add `confit export` and `[env]` profiles for one-shot env materialization ([`642d57c`](https://github.com/krondor-corp/confit/commit/642d57c11bc2b0f791b74656d2f5bb103fad966c))
    - Merge pull request #10 from krondor-corp/release-automation ([`d68f2da`](https://github.com/krondor-corp/confit/commit/d68f2dafdcb978e2758e5a22bdcecdd7611f7ca7))
    - Bump confit-core v0.3.0, confit-cli v0.3.0 ([`bc1d471`](https://github.com/krondor-corp/confit/commit/bc1d4714f848e4db18478643edcdeb62db16d8d6))
    - Bump version to 0.3.0 ([`ae29dc0`](https://github.com/krondor-corp/confit/commit/ae29dc00d1affb38a2317d17d6abcc779fb16205))
    - Merge pull request #9 from krondor-corp/feature/kro-160-add-cached-sources-for-bulk-secretenv-loading ([`bb359fb`](https://github.com/krondor-corp/confit/commit/bb359fb34d37e1abde70a4ffee58570a327e3e54))
    - Add [sources] for bulk secret/env loading with lazy caching ([`f974a27`](https://github.com/krondor-corp/confit/commit/f974a2770e03ed1e1cc2cca2b1d996284a161733))
    - Install script unbound variable and cargo fmt ([`c448cf3`](https://github.com/krondor-corp/confit/commit/c448cf344f0ce902c442dd3f243de84466bfc8ae))
    - Merge pull request #1 from krondor-corp/release-automation ([`cfa85af`](https://github.com/krondor-corp/confit/commit/cfa85af8ad448c78ca511be44d8b4747762cce92))
    - Bump confit-core v0.2.0, confit-cli v0.2.0 ([`233ae41`](https://github.com/krondor-corp/confit/commit/233ae4199b3727ee8a0a97c26a32223e1bd30d6e))
    - Initial release of confit ([`3665553`](https://github.com/krondor-corp/confit/commit/36655538885e4feb1debd9a157f240f3c17acae7))
</details>

## v0.4.0 (2026-06-27)

<csr-id-22005abbaa32cecf19bc60b3443f59ba290dfc4d/>
<csr-id-ae29dc00d1affb38a2317d17d6abcc779fb16205/>

### Chore

 - <csr-id-22005abbaa32cecf19bc60b3443f59ba290dfc4d/> bump version to 0.4.0
 - <csr-id-ae29dc00d1affb38a2317d17d6abcc779fb16205/> bump version to 0.3.0

### New Features

 - <csr-id-642d57c11bc2b0f791b74656d2f5bb103fad966c/> add `confit export` and `[env]` profiles for one-shot env materialization
   Materialize a complete, multi-section dev env once (one provider/1Password
   unlock) into a safe file an agent or fresh shell can source repeatedly.
   
   confit export [--profile <name> | <SECTION>...] [OPTIONS]
   - composes multiple sections (later wins on key conflict) or a named
   [env.<name>] profile drawing from sections/providers/sources/literals
   - --out writes 0600, atomically (temp+rename), and refuses non-gitignored
   paths (checked via `git check-ignore`) unless --force; warns outside a repo
   - --format dotenv|shell|json (all shell-safe single-quoted)
   - --reveal required to emit secret values (refuses rather than write masked
   junk); with --out, secrets go only to the file and a keys-only summary
   prints to stderr
   - fails closed: nonzero exit and no file written on any unresolved value
   - --upper / --prefix / --no-eval carried over from show
   
   [env.<name>] profiles compose a named env declared once in config. Profiles can
   pin their own vars via `vars.stage = "..."` (precedence: [vars] < profile vars
   < CONFIT_VAR_* < --set). Profiles and export share a single SourceCache, so a
   [sources] bag referenced across many keys loads only once.
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

 - <csr-id-c448cf344f0ce902c442dd3f243de84466bfc8ae/> install script unbound variable and cargo fmt
   Move tmp dir to global scope so the EXIT trap can access it under
   set -u. Run cargo fmt on all files.

## v0.3.0 (2026-06-08)

<csr-id-ae29dc00d1affb38a2317d17d6abcc779fb16205/>

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

 - <csr-id-c448cf344f0ce902c442dd3f243de84466bfc8ae/> install script unbound variable and cargo fmt
   Move tmp dir to global scope so the EXIT trap can access it under
   set -u. Run cargo fmt on all files.

## v0.2.0 (2026-05-09)

### New Features

 - <csr-id-36655538885e4feb1debd9a157f240f3c17acae7/> initial release of confit
   Config resolver with interpolation, shell evaluation, pluggable providers,
   secret masking, and SSH agent management. Includes CLI, wiki, CI/CD pipelines,
   and 85 tests.

