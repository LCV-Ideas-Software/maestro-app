// Modulo: src-tauri/src/command_path.rs
// Descricao: PATH-resolution helpers for child command spawn extracted from
// lib.rs in v0.3.33 per `docs/code-split-plan.md` migration step 5.
//
// What's here:
//   - `resolve_command` — locates a CLI by name on the effective PATH
//     (absolute and relative paths bypass the search). For bare Windows agy,
//     native executables precede batch shims; other command ordering is unchanged.
//   - `command_candidate_paths` — on Windows, expands a bare `<command>`
//     stem into `[<command>.exe, <command>.cmd, <command>.bat,
//     <command>.ps1, <command>]` so the resolver can match any common
//     extension; on POSIX, returns the path unchanged.
//   - `command_search_dirs` — assembles the effective PATH-like search
//     order: portable `data/bootstrap/npm-user` first, process PATH second,
//     then well-known Windows install locations (USERPROFILE\.cargo\bin,
//     APPDATA\npm, LOCALAPPDATA\agy\bin,
//     LOCALAPPDATA\Programs\nodejs, LOCALAPPDATA\Microsoft\WinGet\Links, C:\npm-global,
//     WinGet ripgrep package dirs, C:\Program Files\nodejs, C:\nvm4w\nodejs,
//     C:\Program Files\GitHub CLI). Deduplicates by
//     case-insensitive path string.
//
// What stays in lib.rs (consumed via `pub(crate)` imports):
//   - `command_check`, `run_resolved_command_with_timeout`,
//     `run_resolved_command_observed`, `read_pipe_to_end_counting_classified`,
//     `classify_pipe_error`, `resolved_command_builder`,
//     `apply_editorial_agent_environment` — the spawn machinery is tightly
//     coupled to editorial orchestration via `CommandProgressContext` /
//     `TimedCommandOutput` / `log_editorial_agent_*` helpers; planned for a
//     follow-up batch when the editorial orchestration core is split.
//
// v0.3.33 is a pure move: every signature, format string, and PATH order
// is identical to the v0.3.32 lib.rs source (commit e149e9c).

use std::collections::BTreeSet;
#[cfg(windows)]
use std::fs;
use std::path::{Path, PathBuf};

use crate::app_paths::data_dir;

pub(crate) fn resolve_command(command: &str) -> Option<PathBuf> {
    let command_path = Path::new(command);
    if command_path.is_absolute() || command.contains('\\') || command.contains('/') {
        return command_candidate_paths(command_path)
            .into_iter()
            .find(|path| path.is_file());
    }

    resolve_command_on_search_path(command, &command_search_dirs())
}

fn resolve_command_on_search_path(command: &str, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    #[cfg(windows)]
    if command.eq_ignore_ascii_case("agy") {
        // The official Windows CLI is native. An older npm/PATH batch shim
        // must not hide it: PTY batch launchers cannot safely carry prompts.
        // Keep directory order among native executables and explicit paths.
        if let Some(path) = search_dirs
            .iter()
            .map(|dir| dir.join(command).with_extension("exe"))
            .find(|path| path.is_file())
        {
            return Some(path);
        }
    }

    search_dirs
        .iter()
        .flat_map(|dir| command_candidate_paths(&dir.join(command)))
        .find(|path| path.is_file())
}

fn command_candidate_paths(path: &Path) -> Vec<PathBuf> {
    if path.extension().is_some() {
        return vec![path.to_path_buf()];
    }

    #[cfg(windows)]
    {
        ["exe", "cmd", "bat", "ps1", ""]
            .into_iter()
            .map(|ext| {
                if ext.is_empty() {
                    path.to_path_buf()
                } else {
                    path.with_extension(ext)
                }
            })
            .collect()
    }

    #[cfg(not(windows))]
    {
        vec![path.to_path_buf()]
    }
}

pub(crate) fn command_search_dirs() -> Vec<PathBuf> {
    // Portable bootstrap installs live with the application data instead of
    // mutating HKCU/HKLM or the persistent Windows PATH.  Search this prefix
    // first so a user-approved portable Claude/Codex install wins over a stale
    // machine-wide executable.
    let mut dirs = vec![data_dir().join("bootstrap").join("npm-user")];
    dirs.extend(
        std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
            .unwrap_or_default(),
    );

    #[cfg(windows)]
    {
        if let Some(user_profile) = std::env::var_os("USERPROFILE") {
            let user_profile = PathBuf::from(user_profile);
            dirs.push(user_profile.join(".cargo").join("bin"));
        }
        if let Some(app_data) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(app_data).join("npm"));
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let local_app_data = PathBuf::from(local_app_data);
            dirs.push(local_app_data.join("agy").join("bin"));
            dirs.push(local_app_data.join("Programs").join("nodejs"));
            dirs.push(
                local_app_data
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links"),
            );
            append_winget_ripgrep_dirs(
                &mut dirs,
                &local_app_data
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Packages"),
            );
        }
        dirs.push(PathBuf::from(r"C:\npm-global"));
        dirs.push(PathBuf::from(r"C:\Program Files\nodejs"));
        dirs.push(PathBuf::from(r"C:\nvm4w\nodejs"));
        dirs.push(PathBuf::from(r"C:\Program Files\GitHub CLI"));
    }

    let mut seen = BTreeSet::new();
    dirs.into_iter()
        .filter(|dir| seen.insert(dir.to_string_lossy().to_ascii_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_npm_prefix_is_first_without_persistent_path_or_registry_changes() {
        let dirs = command_search_dirs();
        assert_eq!(
            dirs.first(),
            Some(&data_dir().join("bootstrap").join("npm-user"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn bare_agy_prefers_native_executable_without_changing_other_resolution() {
        let mut fixture = CommandPathFixture::new();
        let early = fixture.directory("early-shims");
        let first_native = fixture.directory("first-native");
        let later_native = fixture.directory("later-native");
        let early_agy = fixture.file(&early, "agy.cmd");
        let first_agy = fixture.file(&first_native, "agy.exe");
        let later_agy = fixture.file(&later_native, "agy.exe");
        let early_claude = fixture.file(&early, "claude.cmd");
        fixture.file(&first_native, "claude.exe");
        let dirs = [early, first_native.clone(), later_native];

        assert_eq!(
            resolve_command_on_search_path("agy", &dirs),
            Some(first_agy.clone()),
            "an earlier batch shim must not hide the first available native agy executable",
        );
        assert_eq!(
            resolve_command_on_search_path("AGY", &dirs),
            Some(first_native.join("AGY.exe")),
        );
        assert_eq!(
            resolve_command_on_search_path("claude", &dirs),
            Some(early_claude),
            "other commands must preserve the existing directory priority",
        );
        assert_eq!(
            resolve_command_on_search_path("agy.cmd", &dirs),
            Some(early_agy.clone()),
            "an explicitly named extension must retain its existing resolution",
        );
        assert_eq!(
            resolve_command(early_agy.to_str().unwrap()),
            Some(early_agy.clone()),
            "an explicit path must bypass native preference",
        );

        fs::remove_file(first_agy).unwrap();
        assert_eq!(
            resolve_command_on_search_path("agy", &dirs),
            Some(later_agy.clone()),
            "native candidates must retain directory order",
        );
        fs::remove_file(later_agy).unwrap();
        assert_eq!(
            resolve_command_on_search_path("agy", &dirs),
            Some(early_agy.clone()),
            "without a native executable, preserve resolution for the existing safe launch refusal",
        );
        fs::remove_file(early_agy).unwrap();
        assert_eq!(resolve_command_on_search_path("agy", &dirs), None);
    }

    #[cfg(windows)]
    struct CommandPathFixture {
        root: PathBuf,
        directories: Vec<PathBuf>,
        files: Vec<PathBuf>,
    }

    #[cfg(windows)]
    impl CommandPathFixture {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "maestro-command-path-{}-{nonce}",
                std::process::id(),
            ));
            fs::create_dir(&root).unwrap();
            Self {
                root,
                directories: Vec::new(),
                files: Vec::new(),
            }
        }

        fn directory(&mut self, name: &str) -> PathBuf {
            let path = self.root.join(name);
            fs::create_dir(&path).unwrap();
            self.directories.push(path.clone());
            path
        }

        fn file(&mut self, directory: &Path, name: &str) -> PathBuf {
            let path = directory.join(name);
            fs::write(&path, b"resolution-only fixture; never executed").unwrap();
            self.files.push(path.clone());
            path
        }
    }

    #[cfg(windows)]
    impl Drop for CommandPathFixture {
        fn drop(&mut self) {
            for path in &self.files {
                let _ = fs::remove_file(path);
            }
            for path in self.directories.iter().rev() {
                let _ = fs::remove_dir(path);
            }
            let _ = fs::remove_dir(&self.root);
        }
    }
}

#[cfg(windows)]
fn append_winget_ripgrep_dirs(dirs: &mut Vec<PathBuf>, packages_dir: &Path) {
    let Ok(packages) = fs::read_dir(packages_dir) else {
        return;
    };
    for package in packages.flatten() {
        let package_path = package.path();
        let package_name = package_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !package_name.contains("ripgrep") {
            continue;
        }
        if package_path.join("rg.exe").is_file() {
            dirs.push(package_path.clone());
        }
        if let Ok(children) = fs::read_dir(&package_path) {
            for child in children.flatten() {
                let child_path = child.path();
                if child_path.join("rg.exe").is_file() {
                    dirs.push(child_path);
                }
            }
        }
    }
}
