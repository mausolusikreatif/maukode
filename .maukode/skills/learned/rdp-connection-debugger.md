---
name: rdp-connection-debugger
description: "Systematic debugging of RDP connection failures by testing protocol layers"
run_as_subagent: false
---
When debugging RDP connection failures, test in this order: 1) TCP connect without data (checks firewall/port listening), 2) Send random bytes (checks if service accepts connections), 3) Send SSL ClientHello (checks TLS layer), 4) Send X.224 RDP handshake (checks RDP protocol handler). Pattern analysis: TCP refuse = firewall/port issue, TCP accept+random hold = service running, X.224 reset = RDP protocol rejection (check security protocol requirements). Test with FreeRDP using different /sec: flags (tls, rdp, nla) to identify which security protocol Windows accepts.
