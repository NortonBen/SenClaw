use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "senclaw", version, about = "SenClaw — multi-group AI gateway")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start the SenClaw daemon (default when no subcommand is given)
    Start,
    /// Manage local skills
    Skills {
        #[command(subcommand)]
        cmd: senclaw::cli::commands::skills::SkillsCmd,
    },
    /// Interact with ClawHub
    Clawhub {
        #[command(subcommand)]
        cmd: senclaw::cli::commands::clawhub::ClawhubCmd,
    },
    /// Manage Feishu wiki
    Wiki {
        #[command(subcommand)]
        cmd: senclaw::cli::commands::wiki::WikiCmd,
    },
    /// Manage messaging channels
    Channel {
        #[command(subcommand)]
        cmd: senclaw::cli::commands::channel::ChannelCmd,
    },
    /// Run a one-shot disposable agent task (for hook scripts: reflection / summarization / analysis).
    AgentTask(senclaw::cli::commands::agent_task::AgentTaskCmd),

    // ===== MCP servers (spawned as subprocesses by sema-core) =====
    /// Start the schedule MCP server (stdio JSON-RPC)
    ScheduleServer,
    /// Start the workspace MCP server (stdio JSON-RPC)
    WorkspaceServer,
    /// Start the memory MCP server (stdio JSON-RPC)
    MemoryServer,
    /// Start the send MCP server (stdio JSON-RPC)
    SendServer,
    /// Start the dispatch MCP server (stdio JSON-RPC)
    DispatchServer,
    /// Start the virtual agent MCP server (stdio JSON-RPC)
    VirtualServer,
    /// Start the admin MCP server (stdio JSON-RPC)
    AdminServer,
    /// Start the Wiki MCP server — Feishu/Lark (stdio JSON-RPC)
    WikiServer,
    /// Start the browser MCP server (stdio JSON-RPC)
    BrowserServer,
    /// Start the Space MCP server — notes, calendar, email, sync (stdio JSON-RPC)
    SpaceServer,
    /// Start the code knowledge graph MCP server (stdio JSON-RPC)
    CodeGraphServer,
    /// Start the code editing MCP server (stdio JSON-RPC)
    CodeServer,
    /// Start the Litho (deepwiki-rs) MCP server (stdio JSON-RPC)
    LithoServer,
    /// Start the cognitive memory MCP server — graph + Hebbian (stdio JSON-RPC)
    CognitiveServer,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    // MCP servers MUST log to stderr (stdout is reserved for JSON-RPC).
    // For daemon and CLI commands, stdout logging is fine.
    let is_mcp = matches!(
        cli.command,
        Some(
            Command::ScheduleServer
                | Command::WorkspaceServer
                | Command::MemoryServer
                | Command::SendServer
                | Command::DispatchServer
                | Command::VirtualServer
                | Command::AdminServer
                | Command::WikiServer
                | Command::BrowserServer
                | Command::SpaceServer
        | Command::CodeGraphServer
        | Command::CodeServer
        | Command::LithoServer
        | Command::CognitiveServer
        )
    );

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if is_mcp {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    match cli.command.unwrap_or(Command::Start) {
        Command::Start => {
            let mut cfg = senclaw::config::Config::from_env();
            // Settings UI persists embedding choices to global_config.json.
            // Layer them on top of env so the UI actually drives runtime.
            let gcp = cfg.paths.global_config_path.clone();
            cfg.apply_persisted_overrides(&gcp);
            senclaw::run_daemon(cfg).await
        }
        Command::Skills { cmd } => senclaw::cli::commands::skills::run(cmd).await,
        Command::Clawhub { cmd } => senclaw::cli::commands::clawhub::run(cmd).await,
        Command::Wiki { cmd } => senclaw::cli::commands::wiki::run(cmd).await,
        Command::Channel { cmd } => senclaw::cli::commands::channel::run(cmd).await,
        Command::AgentTask(cmd) => senclaw::cli::commands::agent_task::run(cmd).await,

        // MCP servers
        Command::ScheduleServer => senclaw::mcp::schedule_server::run_stdio_server().await,
        Command::WorkspaceServer => senclaw::mcp::workspace_server::run_stdio_server().await,
        Command::MemoryServer => senclaw::mcp::memory_server::run_stdio_server().await,
        Command::SendServer => senclaw::mcp::send_server::run_stdio_server().await,
        Command::DispatchServer => senclaw::mcp::dispatch_server::run_stdio_server().await,
        Command::VirtualServer => senclaw::mcp::virtual_server::run_stdio_server().await,
        Command::AdminServer => senclaw::mcp::admin_server::run_stdio_server().await,
        Command::WikiServer => senclaw::mcp::wiki_server::run_stdio_server().await,
        Command::BrowserServer => senclaw::mcp::browser_server::run_stdio_server().await,
        Command::SpaceServer => senclaw::mcp::space_server::run_stdio_server().await,
        Command::CodeGraphServer => senclaw::mcp::code_graph_server::run_code_graph_server().await,
        Command::CodeServer => senclaw::mcp::code_server::run_code_server().await,
        Command::LithoServer => senclaw::mcp::litho_server::run_stdio_server().await,
        Command::CognitiveServer => senclaw::mcp::cognitive_server::run_stdio_server().await,
    }
}
