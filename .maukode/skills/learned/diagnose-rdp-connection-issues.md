---
name: diagnose-rdp-connection-issues
description: "Systematic diagnosis of RDP connection failures by testing TCP behavior vs protocol handshake"
run_as_subagent: false
---
When diagnosing RDP connection failures, perform these tests in order: 1) Test TCP connect without sending data — if refused, service isn't running; if accepted, service is listening. 2) Send random bytes — if connection holds, TCP layer works; if resets, firewall is blocking. 3) Send X.224 Connection Request (RDP handshake) — if accepted, RDP is ready; if reset while random bytes hold, the RDP protocol handler is corrupted (likely certificate issue or listener misconfiguration). 4) Test from multiple paths (localhost, guest IP, forwarded ports) to isolate network vs protocol issues. Document the pattern: "TCP accept + random hold + X.224 reset = RDP protocol handler corruption, not firewall."
