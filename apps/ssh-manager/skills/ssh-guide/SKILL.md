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
   - Call the `ssh_list_hosts` MCP tool to retrieve all managed SSH hosts.
   - Look through the list and find the host that matches the `name` (or IP) requested by the user.
   - Extract the `id` (Host ID) of the matching host.

2. **Start the SSH Connection**:
   - Use the `ssh_start_connect` MCP tool and provide the `host_id` parameter.
   - The tool will establish a stateful SSH connection and return a unique `connection_id`.

3. **Execute Commands**:
   - Use the `ssh_execute_command` MCP tool to run whatever shell commands the user requested.
   - You MUST provide the `connection_id` you received in Step 2.
   - You can call `ssh_execute_command` multiple times using the same `connection_id` if you need to run several commands in the same session.

4. **Close the Connection**:
   - When you are completely finished with your tasks on the server, use the `ssh_close_connect` MCP tool.
   - Provide the `connection_id` to close the session and free up resources.
