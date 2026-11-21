mod app;
mod config;
mod editor;
mod event;
mod explorer;
mod highlight;
mod input;
mod layout;
mod menu;
mod pty;
mod render;
mod review;
mod review_window;
mod sidebar;
mod term;
mod workspace;

use winit::event_loop::{ControlFlow, EventLoop};

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let event_loop = EventLoop::<event::UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let proxy = event_loop.create_proxy();
    let mut app = app::App::new(proxy, config::Config::default());
    event_loop.run_app(&mut app)?;

    Ok(())
}
