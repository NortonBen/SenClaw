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
    /// Start the Feishu wiki MCP server (stdio JSON-RPC)
    FeishuWikiServer,
    /// Start the browser MCP server (stdio JSON-RPC)
    BrowserServer,
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
                | Command::FeishuWikiServer
                | Command::BrowserServer
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
            let cfg = senclaw::config::Config::from_env();
            senclaw::run_daemon(cfg).await
        }
        Command::Skills { cmd } => senclaw::cli::commands::skills::run(cmd).await,
        Command::Clawhub { cmd } => senclaw::cli::commands::clawhub::run(cmd).await,
        Command::Wiki { cmd } => senclaw::cli::commands::wiki::run(cmd).await,
        Command::Channel { cmd } => senclaw::cli::commands::channel::run(cmd).await,

        // MCP servers
        Command::ScheduleServer => senclaw::mcp::schedule_server::run_stdio_server().await,
        Command::WorkspaceServer => senclaw::mcp::workspace_server::run_stdio_server().await,
        Command::MemoryServer => senclaw::mcp::memory_server::run_stdio_server().await,
        Command::SendServer => senclaw::mcp::send_server::run_stdio_server().await,
        Command::DispatchServer => senclaw::mcp::dispatch_server::run_stdio_server().await,
        Command::VirtualServer => senclaw::mcp::virtual_server::run_stdio_server().await,
        Command::AdminServer => senclaw::mcp::admin_server::run_stdio_server().await,
        Command::FeishuWikiServer => senclaw::mcp::feishu_wiki_server::run_stdio_server().await,
        Command::BrowserServer => senclaw::mcp::browser_server::run_stdio_server().await,
    }
}
