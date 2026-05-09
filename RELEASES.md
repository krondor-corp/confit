# Releases & Pages Setup

## Release Pipeline

Three workflows chain together to automate releases:

1. **Push to `main`** → `release-pr.yml` scans conventional commits, bumps version, opens a release PR
2. **Merge the release PR** → `release-tag.yml` creates a `v*` tag
3. **Tag pushed** → `release.yml` builds binaries (linux x86_64/aarch64, macOS x86_64/aarch64), creates a GitHub Release

### PAT Setup

The default `GITHUB_TOKEN` can't push tags that trigger other workflows (GitHub's anti-cascade rule). You need a fine-grained PAT:

1. Go to **GitHub → Settings → Developer settings → Personal access tokens → Fine-grained tokens**
2. Create a token:
   - **Name:** `krondor-corp-confit-release`
   - **Resource owner:** `krondor-corp`
   - **Repository access:** Only select repositories → `krondor-corp/confit`
   - **Permissions:** Contents → Read and write
3. Go to **repo Settings → Secrets and variables → Actions**
4. Add a repository secret: **Name:** `RELEASE_PAT`, **Value:** the token

### Cutting a Release

Just push to `main` with conventional commits:

- `feat: ...` → minor bump
- `fix: ...` → patch bump
- `feat!: ...` or `BREAKING CHANGE` → major bump

The release PR opens automatically. Merge it to trigger the build.

### Manual Release

Trigger from the Actions tab: **Release → Run workflow → enter tag (e.g. `v0.2.0`)**

## GitHub Pages

The wiki deploys automatically on push to `main` when files in `wiki/` change.

### Enable Pages

1. Go to **repo Settings → Pages**
2. **Source:** GitHub Actions
3. Save

### Custom Domain (confit.krondor.org)

**DNS:**

Add a CNAME record in your DNS provider:

```
confit.krondor.org → krondor-corp.github.io
```

**GitHub:**

1. Go to **repo Settings → Pages → Custom domain**
2. Enter `confit.krondor.org`
3. Save — GitHub will verify DNS and provision TLS
4. Check **Enforce HTTPS**

**CNAME file:**

Create `wiki/CNAME` so Jekyll includes it in the build:

```
confit.krondor.org
```

This file gets copied to `_site/` during build so GitHub Pages knows the custom domain on every deploy.

### Verify

After DNS propagates (usually a few minutes):

```bash
curl -I https://confit.krondor.org
```

Should return `200` with the wiki homepage.
