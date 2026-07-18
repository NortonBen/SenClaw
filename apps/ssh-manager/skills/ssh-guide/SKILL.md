---
name: ssh-guide
description: A guide for AI agents on how to connect to SSH servers using the Stateful SSH MCP.
triggers:
  - "hướng dẫn ssh"
  - "ssh guide"
  - "cách dùng ssh"
---

# SSH Connection Guide

When the user asks you to "connect to SSH server", "run a command on [Server Name]", or "ssh into [Server Name]", follow these instructions:

## Instructions

1. **Find the Target Host**:
   - Call the `mcp__ssh-manager-mcp__ssh_list_hosts` MCP tool to retrieve all managed SSH hosts.
   - Look through the list and find the host that matches the `name` (or IP) requested by the user.
   - Extract the `id` (Host ID) of the matching host.

2. **Start the SSH Connection**:
   - **Saved host (found in Step 1):** use the `mcp__ssh-manager-mcp__ssh_start_connect_id` MCP tool with `host_id` = the exact `id` value from `ssh_list_hosts` (NOT the name or IP).
   - **Unsaved server (user gave you raw connection details):** use the `mcp__ssh-manager-mcp__ssh_start_connect` MCP tool with explicit `host` (IP/hostname) and `user`; `port` is optional (default 22), `password` is optional.
   - Either tool establishes a stateful SSH connection and returns a unique `connection_id`.

3. **Execute Commands**:
   - Use the `mcp__ssh-manager-mcp__ssh_execute_command` MCP tool to run whatever shell commands the user requested.
   - You MUST provide the `connection_id` you received in Step 2.
   - You can call `mcp__ssh-manager-mcp__ssh_execute_command` multiple times using the same `connection_id` if you need to run several commands in the same session.

4. **Close the Connection**:
   - When you are completely finished with your tasks on the server, use the `mcp__ssh-manager-mcp__ssh_close_connect` MCP tool.
   - Provide the `connection_id` to close the session and free up resources.
