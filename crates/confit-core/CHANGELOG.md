# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
 - <csr-id-88ad30561c48d197afa49ae7e78d571fa9731c60/> resolve() ran the provider twice for a single scalar value
   resolve() called resolve_provider() on the top-level string just to
   learn its secret flag, then called resolve_providers() on the same
   value to produce the result -- which, for a plain string, dispatched
   the identical provider/source URI a second time. Every scalar
   `confit resolve` through a provider executed the provider command
   twice (present since the initial release; providers with side effects
   or prompts, e.g. `op read`, paid it doubly).
   
   resolve() now branches on the value's shape after shell eval: a bare
   string goes through resolve_provider() once and uses both the value
   and the secret flag from that single call; arrays still walk through
   resolve_providers(); other scalars never touch providers at all.
   Regression test pins the invocation count by having a provider append
   to a file each time it runs.
 - <csr-id-9c3209f9e557adcb3d4304cf64ce645901eba4aa/> expose ResolvedPorts publicly instead of leaving it private
   ResolvedPorts existed but was private, so every caller outside
   ports.rs -- including confit-cli's validate command and confit-core's
   own tests -- was still stuck doing config::get(cfg, "ports.branch")
   + .as_str()/.as_integer() by hand.
 - <csr-id-e730e1f8f4143e3f7444b0b95ab917098be6bb98/> CI ephemeral-port test failure; make [ports] parsing type-safe
   CI failure: two check_host tests used band 43000/4300*10, which falls
   inside Linux's default ephemeral port range (32768-60999) but not
   macOS's -- passed locally, failed on the Linux runner. Moved those
   fixtures to band 20000, below both platforms' default ranges.
   
   Also replaces the hand-rolled toml::Value field access throughout
   ports.rs (.as_table()/.get()/.as_integer() chains for every field,
   twice -- once for the raw [ports] table, once for the expanded one)
   with two serde structs: PortsSpec (input) and ResolvedPorts (output).
   expand_ports and check_host each do one typed deserialize instead of
   manually walking table fields by string key, so a malformed [ports]
   section fails with a normal serde error instead of drifting silently.
   Public function signatures (Value in, Value/Vec<HostIssue> out) are
   unchanged since the rest of confit.toml's resolution pipeline is still
   generic Value all the way down -- this only fixes the ports-owned
   internals, not the whole-config model.
 - <csr-id-637b906648e0c282821e58b6a9ba50ba9f07a7a4/> stop parsing confit.toml twice in confit validate
   validate() already built a BuiltConfig internally; the CLI command
   built a second, independent one just to reach bc.config/bc.config_dir
   for the ports host check. Split validate() into a thin wrapper plus
   validate_built(bc: &BuiltConfig), and have the CLI build once and
   share it for both the section walk and the ports check.
 - <csr-id-8d2b7bbb7e6c49a470bde398b5afbc5d58906376/> fix clippy warnings (map_or, too_many_arguments)
 - <csr-id-4c7572b2d286cd91463ff04fa7ff68522caef908/> fix rustfmt formatting
 - <csr-id-c448cf344f0ce902c442dd3f243de84466bfc8ae/> install script unbound variable and cargo fmt
   Move tmp dir to global scope so the EXIT trap can access it under
   set -u. Run cargo fmt on all files.

### Refactor

 - <csr-id-c49154657078ab6b1896bca79a23e50387999c8c/> share one SourceCache per Config; use Config::resolve in its own tests
   Every top-level Config method (resolve, env, env_multi, validate,
   yaml_section, load) created a fresh SourceCache, so a source's load
   command re-ran on every call even against the same Config -- calling
   .resolve() twice, or .resolve() then .env(), on paths backed by the
   same source loaded it twice. Moved the cache onto Config itself
   (RefCell<SourceCache>, populated lazily on first reference, same as
   before) so it's shared across every call on one Config instance for
   its lifetime. env_with_cache's only reason to exist (threading an
   external cache through env_multi) is gone, so it's folded into env().
   
   Also fixed five tests (end_to_end_resolve, end_to_end_with_file_provider,
   end_to_end_with_ports, end_to_end_with_shell, env_output,
   source_end_to_end_via_config) that predated Config::resolve()/env()
   and manually re-chained get() + interpolate_node() +
   eval_shell()/resolve_provider() instead of just calling the method
   that already does that. Added a test asserting the cache-sharing
   behavior directly (two .resolve() calls and one .env() call against a
   source whose load command produces a fresh value each time it
   actually runs all see the same value).
 - <csr-id-d4145144e91b06af74487ef7840e67cca3d53dbc/> split config.rs into a module directory
   config.rs had grown to ~2000 lines covering four fairly separate
   concerns. Split into crates/confit-core/src/config/:
   
   - interpolate.rs -- the {ref} engine: get(), interpolate_value/node,
     REF_RE.
   - shell.rs -- the $(...) engine: eval_shell/eval_shells, SHELL_RE.
   - providers.rs -- scheme:// dispatch: ProviderSpec, SourceSpec,
     SourceCache, resolve_provider/resolve_providers and their private
     helpers (expand_template, run_shell, parse_dotenv, is_source, etc).
   - mod.rs -- Config itself: build(), the high-level methods
     (resolve/keys/env/env_multi/validate/yaml_section/load), Resolved,
     EnvPair, mask_secrets, find_config/load_raw/collect_env_vars.
   
   Public API unchanged -- confit_core::config::{Config, EnvPair, get,
   interpolate_value, interpolate_node, eval_shell, eval_shells,
   resolve_provider, resolve_providers, ProviderSpec, SourceSpec,
   SourceCache} are all still reachable at the same paths via `pub use`
   in mod.rs; nothing outside confit-core referenced anything else in
   this file. Tests moved with the code they test (each submodule keeps
   its own #[cfg(test)] mod), so `cargo test` output now reads
   config::interpolate::tests::*, config::providers::tests::*,
   config::shell::tests::*, config::tests::* instead of one flat
   config::tests::* block covering all four concerns. Same 84 tests,
   same assertions, no behavior change.
 - <csr-id-8c28e1fa0641f83445777b5ece67c12c4a22e536/> type providers/sources instead of poking raw toml::Value
   [providers.<scheme>] and [sources.<name>] each accept a bare-string
   shorthand or a table (cmd/load + optional secret) in TOML. Every place
   that touched them -- resolve_provider, load_source_data,
   resolve_from_source, source_is_secret, is_source -- did its own
   .as_table()/.get("cmd"/"load"/"secret")/.as_str()/.as_bool() walk to
   pull that shape back out.
   
   Replaced with two serde types deserialized directly from the raw
   Value:
   
     enum ProviderSpec { Shorthand(String), Full { cmd: String } }
     enum SourceSpec { Shorthand(String), Full { load: String, secret: bool } }
   
   Each gets a method that does the actual work instead of a free
   function reaching into fields: ProviderSpec::resolve() runs the cmd
   template, SourceSpec::load() runs the load command once and parses its
   dotenv-format output. Config.providers/sources are now
   HashMap<String, ProviderSpec>/HashMap<String, SourceSpec>, parsed (and
   shape-validated) once in Config::build instead of on every dispatch.
   
   resolve_provider/resolve_providers stay free functions with typed
   params -- directly unit-testable without building a whole Config --
   and Config gets thin resolve_provider/resolve_providers methods that
   supply providers/sources/merged_vars/config_dir automatically, so the
   5 methods that dispatch through them (resolve, env, validate,
   yaml_section, load) stopped repeating those 4 args at every call site.
   
   Also fixed a line-continuation string that got mangled into a run of
   literal spaces during an earlier heredoc-based rewrite of this file
   (cosmetic in the error text, not a behavior change).
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

 - 29 commits contributed to the release.
 - 19 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Bump version to 0.5.0 ([`38d5a6c`](https://github.com/krondor-corp/confit/commit/38d5a6ceba3de599ca78751d7ba6d3e67966e0c9))
    - Merge pull request #15 from krondor-corp/feature/dev-ports ([`5cf86ed`](https://github.com/krondor-corp/confit/commit/5cf86ed2f3c82f3b0c01b259b987a6935438dee8))
    - Apply branch-audit findings across slots, git, ports, and config ([`bcfc202`](https://github.com/krondor-corp/confit/commit/bcfc202e1cd90dcfd0d5e0e16917d508b93c8a9a))
    - Resolve() ran the provider twice for a single scalar value ([`88ad305`](https://github.com/krondor-corp/confit/commit/88ad30561c48d197afa49ae7e78d571fa9731c60))
    - Address review: move ports/slots into config, drop branch field, truncate slug ([`5fda4e9`](https://github.com/krondor-corp/confit/commit/5fda4e9c1f7cf74527973f94c2517992ec5e8333))
    - Share one SourceCache per Config; use Config::resolve in its own tests ([`c491546`](https://github.com/krondor-corp/confit/commit/c49154657078ab6b1896bca79a23e50387999c8c))
    - Split config.rs into a module directory ([`d414514`](https://github.com/krondor-corp/confit/commit/d4145144e91b06af74487ef7840e67cca3d53dbc))
    - Type providers/sources instead of poking raw toml::Value ([`8c28e1f`](https://github.com/krondor-corp/confit/commit/8c28e1fa0641f83445777b5ece67c12c4a22e536))
    - BuiltConfig -> Config, one build() constructor, methods ([`8c06d4b`](https://github.com/krondor-corp/confit/commit/8c06d4ba2388c623bf37c05330a8fae9e35d4a9a))
    - Expose ResolvedPorts publicly instead of leaving it private ([`9c3209f`](https://github.com/krondor-corp/confit/commit/9c3209f9e557adcb3d4304cf64ce645901eba4aa))
    - CI ephemeral-port test failure; make [ports] parsing type-safe ([`e730e1f`](https://github.com/krondor-corp/confit/commit/e730e1f8f4143e3f7444b0b95ab917098be6bb98))
    - Stop parsing confit.toml twice in confit validate ([`637b906`](https://github.com/krondor-corp/confit/commit/637b906648e0c282821e58b6a9ba50ba9f07a7a4))
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
    - Fix clippy warnings (map_or, too_many_arguments) ([`8d2b7bb`](https://github.com/krondor-corp/confit/commit/8d2b7bbb7e6c49a470bde398b5afbc5d58906376))
    - Fix rustfmt formatting ([`4c7572b`](https://github.com/krondor-corp/confit/commit/4c7572b2d286cd91463ff04fa7ff68522caef908))
    - Add [sources] for bulk secret/env loading with lazy caching ([`f974a27`](https://github.com/krondor-corp/confit/commit/f974a2770e03ed1e1cc2cca2b1d996284a161733))
    - Install script unbound variable and cargo fmt ([`c448cf3`](https://github.com/krondor-corp/confit/commit/c448cf344f0ce902c442dd3f243de84466bfc8ae))
    - Merge pull request #1 from krondor-corp/release-automation ([`cfa85af`](https://github.com/krondor-corp/confit/commit/cfa85af8ad448c78ca511be44d8b4747762cce92))
    - Bump confit-core v0.2.0, confit-cli v0.2.0 ([`233ae41`](https://github.com/krondor-corp/confit/commit/233ae4199b3727ee8a0a97c26a32223e1bd30d6e))
    - Initial release of confit ([`3665553`](https://github.com/krondor-corp/confit/commit/36655538885e4feb1debd9a157f240f3c17acae7))
</details>

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

 - <csr-id-8d2b7bbb7e6c49a470bde398b5afbc5d58906376/> fix clippy warnings (map_or, too_many_arguments)
 - <csr-id-4c7572b2d286cd91463ff04fa7ff68522caef908/> fix rustfmt formatting
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

 - <csr-id-8d2b7bbb7e6c49a470bde398b5afbc5d58906376/> fix clippy warnings (map_or, too_many_arguments)
 - <csr-id-4c7572b2d286cd91463ff04fa7ff68522caef908/> fix rustfmt formatting
 - <csr-id-c448cf344f0ce902c442dd3f243de84466bfc8ae/> install script unbound variable and cargo fmt
   Move tmp dir to global scope so the EXIT trap can access it under
   set -u. Run cargo fmt on all files.

## v0.2.0 (2026-05-09)

### New Features

 - <csr-id-36655538885e4feb1debd9a157f240f3c17acae7/> initial release of confit
   Config resolver with interpolation, shell evaluation, pluggable providers,
   secret masking, and SSH agent management. Includes CLI, wiki, CI/CD pipelines,
   and 85 tests.

