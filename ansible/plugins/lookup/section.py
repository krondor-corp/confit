DOCUMENTATION = """
    name: section
    short_description: One confit.toml section, as a dict
    description:
      - Resolves sections of the nearest confit.toml with
        C(confit show --yaml --reveal), on the controller, cached per run.
      - The confit vars plugin uses this to load C(confit.<section>) lazily;
        it can also be called directly.
    options:
      _terms:
        description: Section names.
        required: true
      root:
        description: Directory holding confit.toml. Defaults to the nearest
          one above the playbook directory, then the working directory.
        type: str
"""

EXAMPLES = """
- ansible.builtin.debug:
    msg: "{{ lookup('krondor_corp.confit.section', 'project').admin_user }}"
"""

from pathlib import Path

from ansible.errors import AnsibleError
from ansible.plugins.lookup import LookupBase
from ansible_collections.krondor_corp.confit.plugins.module_utils.confit import find_root, section


class LookupModule(LookupBase):
    def run(self, terms, variables=None, **kwargs):
        self.set_options(var_options=variables, direct=kwargs)
        root = self.get_option("root")
        if not root:
            starts = [Path.cwd()]
            if variables and variables.get("playbook_dir"):
                starts.insert(0, variables["playbook_dir"])
            root = find_root(*starts)
            if root is None:
                raise AnsibleError("confit: no confit.toml above the playbook or working directory")
        return [section(root, name) for name in terms]
