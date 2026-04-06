"""
venv — Creation of virtual environments.

This module provides the `EnvBuilder` class and a `create` convenience function
for creating lightweight virtual environments.

In Ferrython, `ferrython -m venv <dir>` is the recommended way to create
virtual environments. This module provides the Python API.
"""

import os
import sys
import types

class EnvBuilder:
    """Context manager and builder for virtual environments."""

    def __init__(self, *, system_site_packages=False, clear=False,
                 symlinks=False, upgrade=False, with_pip=False,
                 prompt=None, upgrade_deps=False):
        self.system_site_packages = system_site_packages
        self.clear = clear
        self.symlinks = symlinks
        self.upgrade = upgrade
        self.with_pip = with_pip
        self.prompt = prompt
        self.upgrade_deps = upgrade_deps

    def create(self, env_dir):
        """Create a virtual environment in a directory."""
        env_dir = os.path.abspath(env_dir)
        context = self.ensure_directories(env_dir)
        self.setup_python(context)
        self.create_configuration(context)
        self.setup_scripts(context)
        if self.with_pip:
            self.install_pip(context)
        return context

    def ensure_directories(self, env_dir):
        """Create the directories for the venv."""
        if self.clear and os.path.exists(env_dir):
            import shutil
            shutil.rmtree(env_dir)

        context = types.SimpleNamespace()
        context.env_dir = env_dir
        context.env_name = os.path.basename(env_dir)
        context.prompt = self.prompt or context.env_name

        if os.name == 'nt':
            context.bin_path = os.path.join(env_dir, 'Scripts')
            context.bin_name = 'Scripts'
        else:
            context.bin_path = os.path.join(env_dir, 'bin')
            context.bin_name = 'bin'

        context.lib_path = os.path.join(env_dir, 'lib', 'ferrython')
        context.site_packages = os.path.join(context.lib_path, 'site-packages')
        context.include_path = os.path.join(env_dir, 'include', 'ferrython')
        context.cfg_path = os.path.join(env_dir, 'pyvenv.cfg')
        context.env_exe = os.path.join(context.bin_path, 'ferrython')

        for d in [context.bin_path, context.site_packages, context.include_path]:
            os.makedirs(d, exist_ok=True)

        return context

    def setup_python(self, context):
        """Set up the ferrython executable in the venv."""
        src = sys.executable or 'ferrython'
        dst = context.env_exe

        if self.symlinks:
            try:
                os.symlink(src, dst)
            except (OSError, NotImplementedError):
                import shutil
                shutil.copy2(src, dst)
        else:
            import shutil
            shutil.copy2(src, dst)

        # Create python/python3 symlinks
        for name in ['python', 'python3']:
            link = os.path.join(context.bin_path, name)
            if not os.path.exists(link):
                try:
                    os.symlink(dst, link)
                except (OSError, NotImplementedError):
                    pass

    def create_configuration(self, context):
        """Create pyvenv.cfg."""
        lines = [
            'home = {}'.format(os.path.dirname(sys.executable or 'ferrython')),
            'include-system-site-packages = {}'.format(
                'true' if self.system_site_packages else 'false'
            ),
            'version = 3.11.0',
            'implementation = ferrython',
            'executable = {}'.format(sys.executable or 'ferrython'),
        ]
        if self.prompt and self.prompt != context.env_name:
            lines.append('prompt = {}'.format(self.prompt))

        with open(context.cfg_path, 'w') as f:
            f.write('\n'.join(lines) + '\n')

    def setup_scripts(self, context):
        """Write activation scripts."""
        _write_activate_bash(context)
        _write_activate_fish(context)

    def install_pip(self, context):
        """Install pip into the venv (no-op for Ferrython — ferryip is built-in)."""
        pass


def create(env_dir, *, system_site_packages=False, clear=False,
           symlinks=False, with_pip=False, prompt=None):
    """Create a virtual environment in a directory.

    This is a convenience wrapper around EnvBuilder.
    """
    builder = EnvBuilder(
        system_site_packages=system_site_packages,
        clear=clear,
        symlinks=symlinks,
        with_pip=with_pip,
        prompt=prompt,
    )
    builder.create(env_dir)


def _write_activate_bash(context):
    """Write bash/zsh activation script."""
    script = '''# This file must be used with "source bin/activate" *from bash*
deactivate () {{
    if [ -n "${{_OLD_VIRTUAL_PATH:-}}" ] ; then
        PATH="${{_OLD_VIRTUAL_PATH:-}}"
        export PATH
        unset _OLD_VIRTUAL_PATH
    fi
    if [ -n "${{_OLD_VIRTUAL_PS1:-}}" ] ; then
        PS1="${{_OLD_VIRTUAL_PS1:-}}"
        export PS1
        unset _OLD_VIRTUAL_PS1
    fi
    unset VIRTUAL_ENV
    unset VIRTUAL_ENV_PROMPT
    if [ ! "${{1:-}}" = "nondestructive" ] ; then
        unset -f deactivate
    fi
}}

deactivate nondestructive

VIRTUAL_ENV="{env_dir}"
export VIRTUAL_ENV

_OLD_VIRTUAL_PATH="$PATH"
PATH="{bin_path}:$PATH"
export PATH

_OLD_VIRTUAL_PS1="${{PS1:-}}"
PS1="({prompt}) ${{PS1:-}}"
export PS1

VIRTUAL_ENV_PROMPT="({prompt}) "
export VIRTUAL_ENV_PROMPT
'''.format(env_dir=context.env_dir, bin_path=context.bin_path, prompt=context.prompt)

    path = os.path.join(context.bin_path, 'activate')
    with open(path, 'w') as f:
        f.write(script)
    try:
        os.chmod(path, 0o755)
    except OSError:
        pass


def _write_activate_fish(context):
    """Write fish activation script."""
    script = '''function deactivate -d "Exit virtual environment"
    if set -q _OLD_VIRTUAL_PATH
        set -gx PATH $_OLD_VIRTUAL_PATH
        set -e _OLD_VIRTUAL_PATH
    end
    set -e VIRTUAL_ENV
    set -e VIRTUAL_ENV_PROMPT
end

deactivate

set -gx VIRTUAL_ENV "{env_dir}"
set -gx _OLD_VIRTUAL_PATH $PATH
set -gx PATH "{bin_path}" $PATH
set -gx VIRTUAL_ENV_PROMPT "({prompt}) "
'''.format(env_dir=context.env_dir, bin_path=context.bin_path, prompt=context.prompt)

    path = os.path.join(context.bin_path, 'activate.fish')
    with open(path, 'w') as f:
        f.write(script)
