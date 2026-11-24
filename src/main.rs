#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod agent_window;
mod agents;
mod app;
mod complete;
mod config;
mod diff_view;
mod editor;
mod event;
mod explorer;
mod forge;
mod git;
mod git_status;
mod git_window;
mod highlight;
mod input;
mod layout;
mod menu;
mod process;
mod pty;
mod render;
mod review;
mod review_window;
mod shell;
mod sidebar;
mod term;
mod workspace;

use winit::event_loop::{ControlFlow, EventLoop};

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let event_loop = EventLoop::<event::UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let proxy = event_loop.create_proxy();
    let mut app = app::App::new(proxy, config::Config::load());
    event_loop.run_app(&mut app)?;

    Ok(())
}
