DOCUMENTATION = """
    name: confit
    short_description: confit.toml as the C(confit) variable
    description:
      - Exposes the nearest confit.toml (found from the inventory or playbook
        directory, then the working directory, walking up) as one variable,
        C(confit), on every host.
      - "Use it in templates: C({{ confit.project.admin_user }}), or a whole
        section as a dict: C({{ confit.credentials.ssh }})."
      - Each section is resolved on the controller, with
        C(confit show --yaml --reveal), the first time a play uses it, so a
        playbook needs only the providers (e.g. 1Password vaults) of the
        sections it reads, and one that reads none doesn't need confit at all.
      - Values are revealed. Put no_log on tasks that would print a secret.
    notes:
      - Must be enabled, e.g. C(vars_plugins_enabled = host_group_vars, krondor_corp.confit.confit).
"""

import json
from pathlib import Path

from ansible.plugins.vars import BaseVarsPlugin
from ansible_collections.krondor_corp.confit.plugins.module_utils.confit import find_root, sections, serve

try:
    from ansible.template import trust_as_template
except ImportError:  # ansible-core < 2.19 templates every string
    def trust_as_template(value):
        return value


class VarsModule(BaseVarsPlugin):
    REQUIRES_ENABLED = True

    def get_vars(self, loader, path, entities, cache=True):
        super().get_vars(loader, path, entities)
        # Attach to the `all` group, so every host gets it at group_vars/all precedence.
        if not any(getattr(entity, "name", None) == "all" for entity in entities):
            return {}
        root = find_root(path, Path.cwd())
        if root is None:
            return {}
        serve()
        # Each section is a template that calls the section lookup, so it is
        # only resolved when something reads it.
        return {
            "confit": {
                name: trust_as_template(
                    "{{ lookup('krondor_corp.confit.section', %s, root=%s) }}"
                    % (json.dumps(name), json.dumps(str(root)))
                )
                for name in sections(root)
            }
        }
