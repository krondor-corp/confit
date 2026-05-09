---
title: Recipes
slug: recipes
order: 8
---

## deploy a service with kamal

The pedantic way -- resolve each value by hand and wire them explicitly:

```bash
#!/bin/bash
set -euo pipefail

SERVICE="$1"; shift

SERVER_IP=$(confit resolve credentials.server.ip --reveal)
DOCKER_HUB_TOKEN=$(confit resolve credentials.cloud.docker_hub_token --reveal)

confit ssh --key credentials.ssh.deploy -- \
  env SERVER_IP="$SERVER_IP" DOCKER_HUB_TOKEN="$DOCKER_HUB_TOKEN" \
    kamal "$@" -c "config/deploy/${SERVICE}.yml"
```

Or let confit handle it -- `ssh` loads the key, `run` injects all cloud credentials as env vars:

```bash
#!/bin/bash
set -euo pipefail

SERVICE="$1"; shift

confit ssh --key credentials.ssh.deploy -- \
  confit run credentials.cloud --upper -- \
    kamal "$@" -c "config/deploy/${SERVICE}.yml"
```

## iterate over services

```bash
for svc in $(confit keys services); do
  confit log "checking $svc"
  domain=$(confit resolve "services.${svc}.domain")
  port=$(confit resolve "services.${svc}.port")
  echo "$svc -> $domain:$port"
done
```

## source env in a shell script

```bash
eval "$(confit show services.web.env --export --upper --reveal)"
echo "BASE_URL is $BASE_URL"
```

## run ansible with ssh-agent

```bash
server_ip=$(confit resolve credentials.server.ip --reveal)

confit ssh --key credentials.ssh.admin.private_key -- \
  ansible-playbook playbooks/bootstrap.yml \
    -i "${server_ip}," \
    -u admin
```

## validate vault access

Check that all credentials can be resolved before running a deploy:

```bash
confit log "validating vault access..."
confit validate credentials
confit log --ok "all credentials accessible"
```

## use terraform outputs

```toml
[providers.tf]
cmd = "terraform -chdir=iac/stages/{stage} output -raw {path}"

[vars]
stage = "dev"

[infra]
server_ip = "tf://server_ip"
db_endpoint = "tf://db_endpoint"
cdn_domain = "tf://cdn_domain"
```

```bash
# Dev (default)
$ confit resolve infra.server_ip
10.0.0.1

# Production
$ confit --set stage=production resolve infra.server_ip
65.108.67.19
```

## multi-environment configs

Use variables to switch between environments without separate config files:

```toml
[vars]
stage = "dev"

[providers.op]
cmd = "op read {uri}"

[providers.tf]
cmd = "terraform -chdir=iac/stages/{stage} output -raw {path}"

[vaults]
sites = "myapp-sites-{vars.stage}"

[credentials]
api_key = "secret://op://{vaults.sites}/API_KEY/credential"

[infra]
endpoint = "tf://api_endpoint"
```

```bash
# Dev environment (secret masked)
$ confit resolve credentials.api_key
***

# Production environment (revealed)
$ confit --set stage=production resolve credentials.api_key --reveal
sk-prod-abc123
```

## styled script output

Replace hand-rolled `echo` coloring with `confit log`:

```bash
#!/bin/bash
set -euo pipefail

confit log "starting deploy for $SERVICE"

if deploy_service "$SERVICE"; then
  confit log --ok "deployed $SERVICE"
else
  confit log --err "failed to deploy $SERVICE"
  exit 1
fi
```

Output goes to stderr, so it doesn't interfere with piped data on stdout.
