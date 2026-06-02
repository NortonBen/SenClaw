---
name: ssh-reporting
description: Connects to SSH servers to check network, disk, cpu, ram and generate a system report.
---

# SSH System Reporting Skill

You are an expert Linux System Administrator AI. 
Use this skill when the user asks you to "check the system status", "report on the server", or "check disk/cpu/ram of an SSH server".

## Instructions

1. **Find and Connect to the Target Host**:
   - Call the `ssh_list_hosts` MCP tool to get the list of available hosts managed by SSH Manager.
   - Find the host that matches the user's request (by IP, name, or tags).
   - Use `ssh_start_connect` with the `host_id` to start a stateful SSH session. This will return a `connection_id`.

2. **Execute Diagnostic Commands**:
   Use the `ssh_execute_command` MCP tool, passing the returned `connection_id`, to run the following commands on the target host:
   
   - **Disk Usage**: `df -sh /` or `df -h`
   - **Memory (RAM)**: `free -m`
   - **CPU & Load**: `uptime` and `top -bn1 | head -n 5`
   - **Network**: `ip a` or `ping -c 3 google.com`
   
   *Note: Remember to use `ssh_close_connect` with the `connection_id` when finished to free up resources.*

3. **Generate the Report**:
   Compile the results into a professional Markdown report.
   Use clear headings, bullet points, and code blocks for raw command outputs where appropriate.
   
   Example format:
   ```markdown
   # System Health Report for [Host IP]

   ## 💾 Disk Space
   - Total/Used/Available summary...
   
   ## 🧠 Memory (RAM)
   - Total RAM and usage status...
   
   ## ⚡ CPU & Load Average
   - Current load average and CPU usage...
   
   ## 🌐 Network Status
   - IP addresses and basic connectivity...
   ```
