//! Virtual environment creation — `ferrython -m venv` implementation.

use crate::paths::InstallLayout;
use std::fs;
use std::path::Path;

/// Options for venv creation.
pub struct VenvOptions {
    /// Install pip into the venv (via ensurepip)
    pub with_pip: bool,
    /// Create symlinks instead of copies for the binary
    pub symlinks: bool,
    /// Clear the target directory before creating
    pub clear: bool,
    /// Don't install pip even if with_pip is true
    pub without_pip: bool,
    /// Upgrade the venv scripts (for existing venvs)
    pub upgrade: bool,
    /// Give access to system site-packages
    pub system_site_packages: bool,
    /// Custom prompt string
    pub prompt: Option<String>,
}

impl Default for VenvOptions {
    fn default() -> Self {
        Self {
            with_pip: false,
            symlinks: !cfg!(windows),
            clear: false,
            without_pip: false,
            upgrade: false,
            system_site_packages: false,
            prompt: None,
        }
    }
}

/// Create a virtual environment at `venv_dir`.
pub fn create_venv(venv_dir: &Path, opts: &VenvOptions) -> Result<(), String> {
    let host = InstallLayout::discover();
    let layout = InstallLayout::for_venv(venv_dir, &host);

    // Clear existing if requested
    if opts.clear && venv_dir.exists() {
        fs::remove_dir_all(venv_dir)
            .map_err(|e| format!("Failed to clear {}: {}", venv_dir.display(), e))?;
    }

    // Create directory structure
    fs::create_dir_all(&layout.bin_dir)
        .map_err(|e| format!("mkdir bin: {}", e))?;
    fs::create_dir_all(&layout.site_packages)
        .map_err(|e| format!("mkdir site-packages: {}", e))?;
    fs::create_dir_all(&layout.include_dir)
        .map_err(|e| format!("mkdir include: {}", e))?;

    // Write pyvenv.cfg
    write_pyvenv_cfg(venv_dir, &host, opts)?;

    // Link or copy the ferrython binary
    install_binary(&layout, &host, opts.symlinks)?;

    // Write activation scripts (bash, fish, PowerShell)
    write_activation_scripts(&layout, venv_dir, opts)?;

    // Install pip (ferryip) if requested
    if opts.with_pip && !opts.without_pip {
        install_pip_into_venv(&layout, &host)?;
    }

    Ok(())
}

/// Write the pyvenv.cfg file in the venv root.
fn write_pyvenv_cfg(venv_dir: &Path, host: &InstallLayout, opts: &VenvOptions) -> Result<(), String> {
    let cfg_path = venv_dir.join("pyvenv.cfg");
    let mut content = String::new();

    content.push_str(&format!("home = {}\n", host.bin_dir.display()));
    content.push_str(&format!(
        "include-system-site-packages = {}\n",
        if opts.system_site_packages { "true" } else { "false" }
    ));
    content.push_str("version = 3.11.0\n");
    content.push_str("implementation = ferrython\n");
    content.push_str(&format!(
        "executable = {}\n",
        host.bin_dir.join("ferrython").display()
    ));
    if let Some(ref prompt) = opts.prompt {
        content.push_str(&format!("prompt = {}\n", prompt));
    }

    fs::write(&cfg_path, content)
        .map_err(|e| format!("Write pyvenv.cfg: {}", e))
}

/// Install the ferrython binary into the venv (symlink or copy).
fn install_binary(layout: &InstallLayout, host: &InstallLayout, symlinks: bool) -> Result<(), String> {
    let host_exe = host.bin_dir.join("ferrython");
    let venv_exe = layout.bin_dir.join("ferrython");

    if !host_exe.exists() {
        // Try current_exe as fallback
        let current = std::env::current_exe()
            .map_err(|e| format!("current_exe: {}", e))?;
        if symlinks {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&current, &venv_exe)
                .map_err(|e| format!("symlink: {}", e))?;
            #[cfg(not(unix))]
            fs::copy(&current, &venv_exe)
                .map_err(|e| format!("copy: {}", e))?;
        } else {
            fs::copy(&current, &venv_exe)
                .map_err(|e| format!("copy: {}", e))?;
        }
    } else if symlinks {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&host_exe, &venv_exe)
            .map_err(|e| format!("symlink: {}", e))?;
        #[cfg(not(unix))]
        fs::copy(&host_exe, &venv_exe)
            .map_err(|e| format!("copy: {}", e))?;
    } else {
        fs::copy(&host_exe, &venv_exe)
            .map_err(|e| format!("copy: {}", e))?;
    }

    // Also create python/python3 symlinks for compatibility
    #[cfg(unix)]
    {
        let python_link = layout.bin_dir.join("python");
        let python3_link = layout.bin_dir.join("python3");
        let _ = std::os::unix::fs::symlink(&venv_exe, &python_link);
        let _ = std::os::unix::fs::symlink(&venv_exe, &python3_link);
    }

    Ok(())
}

/// Write bash/fish/PowerShell activation scripts.
fn write_activation_scripts(layout: &InstallLayout, venv_dir: &Path, opts: &VenvOptions) -> Result<(), String> {
    let venv_name = opts.prompt.as_deref().unwrap_or_else(|| {
        venv_dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("venv")
    });
    let bin_path = layout.bin_dir.to_string_lossy();

    // Bash/Zsh activate script
    let activate_bash = format!(
r#"# This file must be used with "source bin/activate" *from bash*
# You cannot run it directly.

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

# Unset irrelevant variables
deactivate nondestructive

VIRTUAL_ENV="{venv_dir}"
export VIRTUAL_ENV

_OLD_VIRTUAL_PATH="$PATH"
PATH="{bin_path}:$PATH"
export PATH

_OLD_VIRTUAL_PS1="${{PS1:-}}"
PS1="({venv_name}) ${{PS1:-}}"
export PS1

VIRTUAL_ENV_PROMPT="({venv_name}) "
export VIRTUAL_ENV_PROMPT
"#,
        venv_dir = venv_dir.display(),
        bin_path = bin_path,
        venv_name = venv_name,
    );

    // Fish activate script
    let activate_fish = format!(
r#"# This file must be used with "source bin/activate.fish" *from fish*

function deactivate -d "Exit virtual environment"
    if set -q _OLD_VIRTUAL_PATH
        set -gx PATH $_OLD_VIRTUAL_PATH
        set -e _OLD_VIRTUAL_PATH
    end
    if set -q _OLD_FISH_PROMPT_OVERRIDE
        set -e _OLD_FISH_PROMPT_OVERRIDE
        functions -e fish_prompt
        if functions -q _old_fish_prompt
            functions -c _old_fish_prompt fish_prompt
            functions -e _old_fish_prompt
        end
    end
    set -e VIRTUAL_ENV
    set -e VIRTUAL_ENV_PROMPT
end

deactivate

set -gx VIRTUAL_ENV "{venv_dir}"
set -gx _OLD_VIRTUAL_PATH $PATH
set -gx PATH "{bin_path}" $PATH

set -gx _OLD_FISH_PROMPT_OVERRIDE 1
set -gx VIRTUAL_ENV_PROMPT "({venv_name}) "

function fish_prompt
    set -l old_status $status
    printf "({venv_name}) "
    builtin echo -n (set_color normal)
end
"#,
        venv_dir = venv_dir.display(),
        bin_path = bin_path,
        venv_name = venv_name,
    );

    fs::write(layout.bin_dir.join("activate"), activate_bash)
        .map_err(|e| format!("Write activate: {}", e))?;
    fs::write(layout.bin_dir.join("activate.fish"), activate_fish)
        .map_err(|e| format!("Write activate.fish: {}", e))?;

    // PowerShell activation script
    let activate_ps1 = format!(
r#"# This file must be used with ". bin/Activate.ps1" from PowerShell.

function global:deactivate ([switch]$NonDestructive) {{
    if (Test-Path variable:_OLD_VIRTUAL_PATH) {{
        $env:PATH = $variable:_OLD_VIRTUAL_PATH
        Remove-Variable "_OLD_VIRTUAL_PATH" -Scope global
    }}
    if (Test-Path variable:_OLD_VIRTUAL_PROMPT) {{
        $function:prompt = $variable:_OLD_VIRTUAL_PROMPT
        Remove-Variable "_OLD_VIRTUAL_PROMPT" -Scope global
    }}
    if (Test-Path env:VIRTUAL_ENV) {{
        Remove-Item env:VIRTUAL_ENV
    }}
    if (Test-Path env:VIRTUAL_ENV_PROMPT) {{
        Remove-Item env:VIRTUAL_ENV_PROMPT
    }}
    if (-not $NonDestructive) {{
        Remove-Item function:deactivate
    }}
}}

deactivate -NonDestructive

$env:VIRTUAL_ENV = "{venv_dir}"
$env:VIRTUAL_ENV_PROMPT = "({venv_name}) "

$variable:_OLD_VIRTUAL_PATH = $env:PATH
$env:PATH = "{bin_path}" + [IO.Path]::PathSeparator + $env:PATH

$variable:_OLD_VIRTUAL_PROMPT = $function:prompt
function global:prompt {{
    Write-Host -NoNewLine -ForegroundColor Green "({venv_name}) "
    & $variable:_OLD_VIRTUAL_PROMPT
}}
"#,
        venv_dir = venv_dir.display(),
        bin_path = bin_path,
        venv_name = venv_name,
    );

    fs::write(layout.bin_dir.join("Activate.ps1"), activate_ps1)
        .map_err(|e| format!("Write Activate.ps1: {}", e))?;

    // Make activate executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(
            layout.bin_dir.join("activate"),
            fs::Permissions::from_mode(0o755),
        );
    }

    Ok(())
}

/// Install ferryip (pip equivalent) into the virtual environment.
fn install_pip_into_venv(layout: &InstallLayout, host: &InstallLayout) -> Result<(), String> {
    // Look for the ferryip binary in the host installation
    let host_ferryip = host.bin_dir.join("ferryip");
    let venv_ferryip = layout.bin_dir.join("ferryip");

    if host_ferryip.exists() {
        // Symlink or copy the ferryip binary
        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(&host_ferryip, &venv_ferryip);
        }
        #[cfg(not(unix))]
        {
            let _ = fs::copy(&host_ferryip, &venv_ferryip);
        }
    } else {
        // Try to find ferryip next to the current executable
        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(exe_dir) = current_exe.parent() {
                let ferryip_nearby = exe_dir.join("ferryip");
                if ferryip_nearby.exists() {
                    #[cfg(unix)]
                    {
                        let _ = std::os::unix::fs::symlink(&ferryip_nearby, &venv_ferryip);
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = fs::copy(&ferryip_nearby, &venv_ferryip);
                    }
                }
            }
        }
    }

    // Also create a pip symlink pointing to ferryip for compatibility
    if venv_ferryip.exists() {
        let pip_link = layout.bin_dir.join("pip");
        let pip3_link = layout.bin_dir.join("pip3");
        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(&venv_ferryip, &pip_link);
            let _ = std::os::unix::fs::symlink(&venv_ferryip, &pip3_link);
        }
        #[cfg(not(unix))]
        {
            let _ = fs::copy(&venv_ferryip, &pip_link);
            let _ = fs::copy(&venv_ferryip, &pip3_link);
        }
    }

    // Write a pip.conf that sets the target to the venv's site-packages
    let pip_conf = format!(
        "[global]\ntarget = {}\n",
        layout.site_packages.display()
    );
    let _ = fs::create_dir_all(layout.prefix.join("pip"));
    let _ = fs::write(layout.prefix.join("pip").join("pip.conf"), pip_conf);

    Ok(())
}
