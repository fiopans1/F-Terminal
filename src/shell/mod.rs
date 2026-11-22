use std::path::PathBuf;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// How the working directory should be handed to the shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CwdMode {
    /// The shell understands the platform-native path.
    Native,
    /// Start through wsl.exe, optionally in a specific distribution.
    #[cfg_attr(not(windows), allow(dead_code))]
    Wsl { distro: Option<String> },
}

/// One launchable shell profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalProfile {
    pub id: String,
    pub name: String,
    /// Compact badge shown in the sidebar picker.
    pub short: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd_mode: CwdMode,
}

/// All detected shells plus the one used by default.
#[derive(Clone, Debug)]
pub struct ShellCatalog {
    pub profiles: Vec<TerminalProfile>,
    pub default_id: String,
}

impl ShellCatalog {
    pub fn detect(preferred: Option<&str>) -> Self {
        let mut profiles = Vec::new();

        #[cfg(unix)]
        unix::detect(&mut profiles, preferred);
        #[cfg(windows)]
        windows::detect(&mut profiles, preferred);
        #[cfg(not(any(unix, windows)))]
        {
            let _ = preferred;
            profiles.push(fallback_profile());
        }

        if profiles.is_empty() {
            profiles.push(fallback_profile());
        }

        if let Some(preferred) = preferred
            && let Some(index) = profiles.iter().position(|profile| {
                profile.id == preferred
                    || profile.name.eq_ignore_ascii_case(preferred)
                    || profile.short.eq_ignore_ascii_case(preferred)
                    || profile
                        .program
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.eq_ignore_ascii_case(preferred)
                                || name
                                    .strip_suffix(".exe")
                                    .is_some_and(|stem| stem.eq_ignore_ascii_case(preferred))
                        })
            })
        {
            let profile = profiles.remove(index);
            profiles.insert(0, profile);
        }

        let default_id = profiles[0].id.clone();
        Self {
            profiles,
            default_id,
        }
    }

    pub fn get(&self, id: &str) -> Option<&TerminalProfile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.profiles.iter().position(|profile| profile.id == id)
    }

    pub fn default_profile(&self) -> &TerminalProfile {
        self.get(&self.default_id)
            .unwrap_or_else(|| &self.profiles[0])
    }
}

/// Translate a Windows path into the Linux path expected by `wsl.exe --cd`.
pub fn wsl_directory(path: &std::path::Path) -> Option<String> {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        for prefix in [r"\\wsl$\", r"\\wsl.localhost\"] {
            if let Some(rest) = value.strip_prefix(prefix) {
                let (_distro, linux) = rest.split_once('\\')?;
                return Some(format!("/{}", linux.replace('\\', "/")));
            }
        }
        path.is_absolute().then(|| value.into_owned())
    }

    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

fn fallback_profile() -> TerminalProfile {
    #[cfg(windows)]
    let program = PathBuf::from("cmd.exe");
    #[cfg(not(windows))]
    let program = PathBuf::from("/bin/sh");

    TerminalProfile {
        id: "fallback".to_owned(),
        name: "Shell".to_owned(),
        short: "sh".to_owned(),
        program,
        args: Vec::new(),
        env: Vec::new(),
        cwd_mode: CwdMode::Native,
    }
}
