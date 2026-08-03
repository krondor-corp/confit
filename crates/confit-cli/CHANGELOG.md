# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.6.0 (2026-08-03)

### Chore

 - <csr-id-abe06b6971dc5c4085bddc5ba6dc26740d39297d/> bump version to 0.6.0
 - <csr-id-38d5a6ceba3de599ca78751d7ba6d3e67966e0c9/> bump version to 0.5.0
 - <csr-id-22005abbaa32cecf19bc60b3443f59ba290dfc4d/> bump version to 0.4.0
 - <csr-id-ae29dc00d1affb38a2317d17d6abcc779fb16205/> bump version to 0.3.0

### New Features

 - <csr-id-6f13ecd82ba9ab872c3a5818b3ccdbeb9e00731d/> add [ports] dev port bands with collision-free per-worktree slots
   Gives a project a fixed 100-port band, fixed ports for shared infra
   within it, and per-worktree ports for HTTP services, so multiple git
   worktrees of the same project can run concurrently against shared
   infra without colliding.
   
   ports.slot is assigned by a stateful ledger (confit-core/src/slots.rs)
   stored at <git-common-dir>/confit/ports.toml, per-clone and shared
   across worktrees, never committed. It proactively prunes branches no
   longer checked out and hands out the lowest free slot, so collisions
   between concurrently-active branches are structurally impossible
   rather than merely unlikely (an earlier hash-based approach could
   collide once enough branches were live).
   
   Also adds `confit validate --host`, checking a resolved [ports]
   section against the host it's running on: collisions, privileged
   ports, ports inside the OS ephemeral range, service ports already
   bound, and ledger corruption.
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

 - <csr-id-bcfc202e1cd90dcfd0d5e0e16917d508b93c8a9a/> apply branch-audit findings across slots, git, ports, and config
 - <csr-id-9c3209f9e557adcb3d4304cf64ce645901eba4aa/> expose ResolvedPorts publicly instead of leaving it private
   ResolvedPorts existed but was private, so every caller outside
   ports.rs -- including confit-cli's validate command and confit-core's
   own tests -- was still stuck doing config::get(cfg, "ports.branch")
   + .as_str()/.as_integer() by hand.
 - <csr-id-637b906648e0c282821e58b6a9ba50ba9f07a7a4/> stop parsing confit.toml twice in confit validate
   validate() already built a BuiltConfig internally; the CLI command
   built a second, independent one just to reach bc.config/bc.config_dir
   for the ports host check. Split validate() into a thin wrapper plus
   validate_built(bc: &BuiltConfig), and have the CLI build once and
   share it for both the section walk and the ports check.
 - <csr-id-9c4bd66152149355b106f891d68e51d4e31c2647/> run [ports] host checks automatically, drop --host flag
   The checks are read-only and cheap, so gating them behind a flag just
   meant people wouldn't discover them. `confit validate` and
   `confit validate ports` now run check_host whenever [ports] is in
   scope; validating an unrelated section (e.g. `confit validate vars`)
   skips it as before.
 - <csr-id-c448cf344f0ce902c442dd3f243de84466bfc8ae/> install script unbound variable and cargo fmt
   Move tmp dir to global scope so the EXIT trap can access it under
   set -u. Run cargo fmt on all files.

### Refactor

 - <csr-id-8c06d4ba2388c623bf37c05330a8fae9e35d4a9a/> BuiltConfig -> Config, one build() constructor, methods
   BuiltConfig is renamed Config and becomes the actual source of truth
   instead of a struct free functions build-and-discard on every call:
   
   - Config::build(path, vars, profile) replaces build_config/
     build_config_layered as the single constructor. It validates that
     every name passed via --set or CONFIT_VAR_* is already declared in
     [vars] (or pinned by the active profile's own vars table) -- catches
     a typo like --set stagee=prod at build time instead of the value
     silently doing nothing. CONFIT_VAR_* is included deliberately, not
     just --set: it's still an explicit, intentional value in the current
     shell, not something to silently ignore.
   - An unknown --profile name is now a build error instead of silently
     resolving to nothing.
   - resolve/keys/env/env_multi/validate/yaml_section/load move from free
     functions (each re-parsing confit.toml internally) to methods on
     Config, so a caller that needs more than one of them (or needs the
     typed ports field alongside a resolve/validate call) builds once and
     shares it. CLI commands (resolve, show, run, export, ssh, keys,
     validate) updated to build one Config and call methods on it.
   - Config gets a `pub ports: Option<ports::ResolvedPorts>` field,
     computed once during build() alongside the generic tree (which still
     gets the same resolved values mirrored in, so {ports.*} interpolation
     is unaffected). ports::resolve() now returns the typed struct
     directly instead of a toml::Value callers had to re-deserialize.
     Callers that need ports data (check_host, tests) read `config.ports`
     instead of re-deriving it from the generic Value tree by hand.
   - Field `config: Value` renamed to `tree: Value` (so it isn't
     `config.config`).

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 26 commits contributed to the release.
 - 14 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Bump version to 0.6.0 ([`abe06b6`](https://github.com/krondor-corp/confit/commit/abe06b6971dc5c4085bddc5ba6dc26740d39297d))
    - Merge pull request #16 from krondor-corp/release-automation ([`9ba3d03`](https://github.com/krondor-corp/confit/commit/9ba3d0365d640e2f37411beca091570e753716e8))
    - Bump confit-core v0.5.0, confit-cli v0.5.0 ([`5b589d8`](https://github.com/krondor-corp/confit/commit/5b589d8898534ee6e3bd0b4bdfa34459c2440869))
    - Bump version to 0.5.0 ([`38d5a6c`](https://github.com/krondor-corp/confit/commit/38d5a6ceba3de599ca78751d7ba6d3e67966e0c9))
    - Merge pull request #15 from krondor-corp/feature/dev-ports ([`5cf86ed`](https://github.com/krondor-corp/confit/commit/5cf86ed2f3c82f3b0c01b259b987a6935438dee8))
    - Apply branch-audit findings across slots, git, ports, and config ([`bcfc202`](https://github.com/krondor-corp/confit/commit/bcfc202e1cd90dcfd0d5e0e16917d508b93c8a9a))
    - Address review: move ports/slots into config, drop branch field, truncate slug ([`5fda4e9`](https://github.com/krondor-corp/confit/commit/5fda4e9c1f7cf74527973f94c2517992ec5e8333))
    - BuiltConfig -> Config, one build() constructor, methods ([`8c06d4b`](https://github.com/krondor-corp/confit/commit/8c06d4ba2388c623bf37c05330a8fae9e35d4a9a))
    - Expose ResolvedPorts publicly instead of leaving it private ([`9c3209f`](https://github.com/krondor-corp/confit/commit/9c3209f9e557adcb3d4304cf64ce645901eba4aa))
    - Stop parsing confit.toml twice in confit validate ([`637b906`](https://github.com/krondor-corp/confit/commit/637b906648e0c282821e58b6a9ba50ba9f07a7a4))
    - Run [ports] host checks automatically, drop --host flag ([`9c4bd66`](https://github.com/krondor-corp/confit/commit/9c4bd66152149355b106f891d68e51d4e31c2647))
    - Add [ports] dev port bands with collision-free per-worktree slots ([`6f13ecd`](https://github.com/krondor-corp/confit/commit/6f13ecd82ba9ab872c3a5818b3ccdbeb9e00731d))
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

## v0.5.0 (2026-08-03)

### Chore

 - <csr-id-38d5a6ceba3de599ca78751d7ba6d3e67966e0c9/> bump version to 0.5.0
 - <csr-id-22005abbaa32cecf19bc60b3443f59ba290dfc4d/> bump version to 0.4.0
 - <csr-id-ae29dc00d1affb38a2317d17d6abcc779fb16205/> bump version to 0.3.0

### New Features

 - <csr-id-6f13ecd82ba9ab872c3a5818b3ccdbeb9e00731d/> add [ports] dev port bands with collision-free per-worktree slots
   Gives a project a fixed 100-port band, fixed ports for shared infra
   within it, and per-worktree ports for HTTP services, so multiple git
   worktrees of the same project can run concurrently against shared
   infra without colliding.
   
   ports.slot is assigned by a stateful ledger (confit-core/src/slots.rs)
   stored at <git-common-dir>/confit/ports.toml, per-clone and shared
   across worktrees, never committed. It proactively prunes branches no
   longer checked out and hands out the lowest free slot, so collisions
   between concurrently-active branches are structurally impossible
   rather than merely unlikely (an earlier hash-based approach could
   collide once enough branches were live).
   
   Also adds `confit validate --host`, checking a resolved [ports]
   section against the host it's running on: collisions, privileged
   ports, ports inside the OS ephemeral range, service ports already
   bound, and ledger corruption.
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

 - <csr-id-bcfc202e1cd90dcfd0d5e0e16917d508b93c8a9a/> apply branch-audit findings across slots, git, ports, and config
 - <csr-id-9c3209f9e557adcb3d4304cf64ce645901eba4aa/> expose ResolvedPorts publicly instead of leaving it private
   ResolvedPorts existed but was private, so every caller outside
   ports.rs -- including confit-cli's validate command and confit-core's
   own tests -- was still stuck doing config::get(cfg, "ports.branch")
   + .as_str()/.as_integer() by hand.
 - <csr-id-637b906648e0c282821e58b6a9ba50ba9f07a7a4/> stop parsing confit.toml twice in confit validate
   validate() already built a BuiltConfig internally; the CLI command
   built a second, independent one just to reach bc.config/bc.config_dir
   for the ports host check. Split validate() into a thin wrapper plus
   validate_built(bc: &BuiltConfig), and have the CLI build once and
   share it for both the section walk and the ports check.
 - <csr-id-9c4bd66152149355b106f891d68e51d4e31c2647/> run [ports] host checks automatically, drop --host flag
   The checks are read-only and cheap, so gating them behind a flag just
   meant people wouldn't discover them. `confit validate` and
   `confit validate ports` now run check_host whenever [ports] is in
   scope; validating an unrelated section (e.g. `confit validate vars`)
   skips it as before.
 - <csr-id-c448cf344f0ce902c442dd3f243de84466bfc8ae/> install script unbound variable and cargo fmt
   Move tmp dir to global scope so the EXIT trap can access it under
   set -u. Run cargo fmt on all files.

### Refactor

 - <csr-id-8c06d4ba2388c623bf37c05330a8fae9e35d4a9a/> BuiltConfig -> Config, one build() constructor, methods
   BuiltConfig is renamed Config and becomes the actual source of truth
   instead of a struct free functions build-and-discard on every call:
   
   - Config::build(path, vars, profile) replaces build_config/
   build_config_layered as the single constructor. It validates that
   every name passed via --set or CONFIT_VAR_* is already declared in
   [vars] (or pinned by the active profile's own vars table) -- catches
   a typo like --set stagee=prod at build time instead of the value
   silently doing nothing. CONFIT_VAR_* is included deliberately, not
   just --set: it's still an explicit, intentional value in the current
   shell, not something to silently ignore.
   - An unknown --profile name is now a build error instead of silently
   resolving to nothing.
   - resolve/keys/env/env_multi/validate/yaml_section/load move from free
   functions (each re-parsing confit.toml internally) to methods on
   Config, so a caller that needs more than one of them (or needs the
   typed ports field alongside a resolve/validate call) builds once and
   shares it. CLI commands (resolve, show, run, export, ssh, keys,
   validate) updated to build one Config and call methods on it.
   - Config gets a `pub ports: Option<ports::ResolvedPorts>` field,
   computed once during build() alongside the generic tree (which still
   gets the same resolved values mirrored in, so {ports.*} interpolation
   is unaffected). ports::resolve() now returns the typed struct
   directly instead of a toml::Value callers had to re-deserialize.
   Callers that need ports data (check_host, tests) read `config.ports`
   instead of re-deriving it from the generic Value tree by hand.
   - Field `config: Value` renamed to `tree: Value` (so it isn't
   `config.config`).

## v0.4.0 (2026-06-27)

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

