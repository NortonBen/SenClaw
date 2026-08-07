"""SenClaw Space App SDK for Python.

A Space App is an ordinary HTTP server the SenClaw daemon launches, health-checks
and reverse-proxies. This package is the four things such an app needs and
nothing else:

* :class:`SenclawSpace` — the daemon's API for this app: settings, its own
  SQLite database, and the AI bridge (the app never holds a provider key).
* :class:`McpServer` — its tools, exposed to agents over MCP.
* :func:`serve` — one HTTP server for health, the UI, the REST API and MCP,
  with the SIGTERM handling an on-demand app needs.
* :mod:`senclaw_space.manifest` — building and checking ``senclaw-manifest.json``.

Standard library only: an app with no dependencies has no install step, so the
daemon starts it in the time Python takes to boot.

A minimal app::

    from senclaw_space import McpServer, SenclawSpace, serve

    space = SenclawSpace()
    mcp = McpServer("demo-mcp")

    @mcp.tool("demo_greet", "Greet someone", {
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"],
    })
    def greet(args):
        return f"Hello, {args['name']}"

    serve(
        {("GET", "/api/status"): lambda req: {"ok": True}},
        health_path="/api/status",
        mcp_path="/api/mcp/sse",
        mcp_handler=mcp.handle,
        static_dir="web",
    )
"""

from .client import (
    KnowledgeHit,
    LlmReply,
    LlmUsage,
    ModelInfo,
    SenclawError,
    SenclawSpace,
    app_id_from_env,
    bind_host,
    port,
)
from .mcp import McpServer, error_content, to_content
from .server import Request, Response, serve

# `dispatch` is deliberately NOT imported here. It is the one module an app
# only needs if it opts into being driven by the dispatcher, and importing it
# eagerly would put its dataclasses in every app's import path for nothing.
# Reach it as `from senclaw_space.dispatch import DispatchProvider`.

__all__ = [
    "KnowledgeHit",
    "LlmReply",
    "LlmUsage",
    "McpServer",
    "ModelInfo",
    "Request",
    "Response",
    "SenclawError",
    "SenclawSpace",
    "app_id_from_env",
    "bind_host",
    "error_content",
    "port",
    "serve",
    "to_content",
]

__version__ = "0.1.0"
