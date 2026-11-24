---
title: Ansible
slug: ansible
order: 10
---

## confit as an ansible variable

The `krondor_corp.confit` collection (in this repo, under `ansible/`) exposes the nearest `confit.toml` to every playbook as one variable, `confit`:

```yaml
- hosts: all
  vars:
    admin_user: "{{ confit.project.admin_user }}"
    ssh_keys: "{{ confit.credentials.ssh }}"   # a whole section, as a dict
```

Install it from git, pinned to a release:

```yaml
# requirements.yml
collections:
  - name: https://github.com/krondor-corp/confit.git#/ansible
    type: git
    version: main  # or a release tag
```

```bash
ansible-galaxy collection install -r requirements.yml
```

and enable the vars plugin in `ansible.cfg`:

```ini
[defaults]
vars_plugins_enabled = host_group_vars, krondor_corp.confit.confit
```

## how it resolves

- `confit.toml` is found by walking up from the inventory or playbook directory, then the working directory.
- Each section is resolved with `confit show --yaml --reveal <section>`, on the controller, the first time a play reads it, and at most once per run. A playbook needs only the providers of the sections it reads; one that reads none doesn't need confit installed.
- Values are revealed. Put `no_log: true` on tasks that would print a secret. `ansible-inventory --list` shows only the unresolved lookups, not the values.

## one-off lookups

The same collection has a lookup, for when you'd rather not enable the vars plugin:

```yaml
admin_user: "{{ lookup('krondor_corp.confit.section', 'project').admin_user }}"
```
