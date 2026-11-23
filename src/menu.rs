use std::path::Path;
use std::process::{Command, Stdio};

use winit::event_loop::EventLoopProxy;

use crate::event::UserEvent;
use crate::process::command as process_command;

/// Actions that can be triggered from the macOS menu or from keyboard
/// shortcuts. Keeping them in one enum avoids duplicating behavior between
/// the native menu and the terminal UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Action {
    Explorer,
    OpenCode,
    CodeReview,
    GitManager,
    Settings,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Action {
    pub fn from_menu_id(id: &str) -> Option<Self> {
        match id {
            "open-explorer" => Some(Self::Explorer),
            "open-code" => Some(Self::OpenCode),
            "code-review" => Some(Self::CodeReview),
            "git-manager" => Some(Self::GitManager),
            "open-settings" => Some(Self::Settings),
            _ => None,
        }
    }
}

pub fn open_explorer(directory: &Path) {
    log::info!("opening file explorer at {}", directory.display());
    spawn_detached(explorer_command(directory), "open explorer");
}

pub fn open_in_code(directory: &Path) {
    log::info!("opening VS Code at {}", directory.display());
    spawn_detached(code_command(directory), "open VS Code");
}

pub fn open_settings() {
    log::info!("settings are not implemented yet");
}

fn explorer_command(directory: &Path) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = process_command("/usr/bin/open");
        cmd.arg(directory);
        cmd
    }

    #[cfg(target_os = "windows")]
    {
        let mut cmd = process_command("explorer.exe");
        cmd.arg(directory);
        cmd
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut cmd = process_command("xdg-open");
        cmd.arg(directory);
        cmd
    }
}

fn code_command(directory: &Path) -> Command {
    #[cfg(target_os = "macos")]
    let program = if Path::new("/usr/local/bin/code").exists() {
        "/usr/local/bin/code"
    } else {
        "code"
    };

    #[cfg(not(target_os = "macos"))]
    let program = "code";

    let mut command = process_command(program);
    command.arg(directory);
    command
}

fn spawn_detached(mut command: Command, description: &str) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if let Err(error) = command.spawn() {
        log::warn!("failed to {description}: {error}");
    }
}

/// Native macOS application menu. On other platforms this is a no-op because
/// Windows/Linux use the in-window controls and keyboard shortcuts.
#[cfg(target_os = "macos")]
pub struct MenuState {
    _menu: muda::Menu,
}

#[cfg(target_os = "macos")]
impl MenuState {
    pub fn install(proxy: EventLoopProxy<UserEvent>) -> anyhow::Result<Self> {
        use muda::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

        let explorer = MenuItem::with_id("open-explorer", "Abrir explorador", true, None);
        let code = MenuItem::with_id("open-code", "Abrir en VS Code", true, None);
        let review = MenuItem::with_id("code-review", "Code Review", true, None);
        let git = MenuItem::with_id("git-manager", "Git", true, None);
        let settings = MenuItem::with_id("open-settings", "Ajustes…", true, None);

        let separator_one = PredefinedMenuItem::separator();
        let separator_two = PredefinedMenuItem::separator();
        let quit = PredefinedMenuItem::quit(Some("Salir"));

        let items: [&dyn IsMenuItem; 8] = [
            &explorer,
            &code,
            &review,
            &git,
            &separator_one,
            &settings,
            &separator_two,
            &quit,
        ];
        let submenu = Submenu::with_items("F-Terminal", true, &items)?;

        let root_items: [&dyn IsMenuItem; 1] = [&submenu];
        let menu = Menu::with_items(&root_items)?;
        menu.init_for_nsapp();
        log::info!("installed native macOS menu");

        let proxy = std::sync::Arc::new(std::sync::Mutex::new(proxy));
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let Some(action) = Action::from_menu_id(&event.id.0) else {
                return;
            };
            if let Ok(proxy) = proxy.lock() {
                let _ = proxy.send_event(UserEvent::Menu(action));
            }
        }));

        Ok(Self { _menu: menu })
    }
}

#[cfg(not(target_os = "macos"))]
pub struct MenuState;

#[cfg(not(target_os = "macos"))]
impl MenuState {
    pub fn install(_proxy: EventLoopProxy<UserEvent>) -> anyhow::Result<Self> {
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_actions() {
        assert_eq!(
            Action::from_menu_id("open-explorer"),
            Some(Action::Explorer)
        );
        assert_eq!(Action::from_menu_id("open-code"), Some(Action::OpenCode));
        assert_eq!(
            Action::from_menu_id("code-review"),
            Some(Action::CodeReview)
        );
        assert_eq!(
            Action::from_menu_id("git-manager"),
            Some(Action::GitManager)
        );
        assert_eq!(
            Action::from_menu_id("open-settings"),
            Some(Action::Settings)
        );
        assert_eq!(Action::from_menu_id("unknown"), None);
    }
}
