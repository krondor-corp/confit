# krondor_corp.confit

Ansible plugins for [confit](https://github.com/krondor-corp/confit).

- `krondor_corp.confit.confit` (vars): the nearest `confit.toml` as the `confit` variable, e.g. `{{ confit.project.admin_user }}`, resolved lazily, once per section per run.
- `krondor_corp.confit.section` (lookup): one section as a dict.

Install from git (pin a release tag once one includes the collection):

```yaml
# requirements.yml
collections:
  - name: https://github.com/krondor-corp/confit.git#/ansible
    type: git
    version: main  # or a release tag
```

Enable the vars plugin:

```ini
[defaults]
vars_plugins_enabled = host_group_vars, krondor_corp.confit.confit
```

See [the ansible docs](https://github.com/krondor-corp/confit/blob/main/wiki/_docs/ansible.md) for details. Requires the `confit` CLI on the controller and ansible-core 2.15+ (Python 3.11+ for `tomllib`).
