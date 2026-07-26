mod app;
mod fixtures;

use app::{AppConfig, GraphDemoApp};
use clap::Parser;
use fixtures::FixtureTopology;
use std::path::PathBuf;
use tracing_subscriber::{filter::Directive, EnvFilter};
use winit::event_loop::{ControlFlow, EventLoop};

#[derive(Parser, Debug)]
#[command(name = "graph-demo", about = "Phoenix native graph renderer")]
struct Arguments {
    #[arg(long, value_enum, default_value_t = FixtureTopology::Clustered)]
    fixture: FixtureTopology,
    #[arg(long, default_value_t = 10_000)]
    nodes: usize,
    #[arg(long, default_value_t = 50_000)]
    edges: usize,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    #[arg(long)]
    json: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let filter = mandatory_runtime_filter(
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,graph_render_wgpu=debug")),
    );
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
    let arguments = Arguments::parse();
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = GraphDemoApp::new(AppConfig {
        topology: arguments.fixture,
        nodes: arguments.nodes,
        edges: arguments.edges,
        seed: arguments.seed,
        json_path: arguments.json,
    });
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn mandatory_runtime_filter(filter: EnvFilter) -> EnvFilter {
    const WGPU_VULKAN_CONVERSION_CEILING: &str = "wgpu_hal::vulkan::conv=error";
    match WGPU_VULKAN_CONVERSION_CEILING.parse::<Directive>() {
        Ok(directive) => filter.add_directive(directive),
        Err(error) => {
            eprintln!("GRAPH_DEMO_RUNTIME_FILTER_INVALID {error}");
            filter
        }
    }
}
