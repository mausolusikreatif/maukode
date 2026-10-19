---
name: rdp-connectivity-diagnosis
description: "Systematic diagnosis of RDP connectivity issues by testing different protocol layers"
run_as_subagent: false
---
When diagnosing RDP connection failures, perform layered testing: 1) Test TCP connect without sending data (distinguishes firewall DROP vs service listening), 2) Send random bytes (checks if service accepts connections generally), 3) Send SSL ClientHello (tests TLS layer), 4) Send X.224 Connection Request with RDP negotiation (tests RDP protocol handler specifically). Pattern interpretation: TCP refused = firewall or no service; TCP accept + random data hold = service running; TCP accept + X.224 RESET = RDP protocol handler broken (likely certificate corruption or listener misconfiguration); TCP accept + X.224 no response = NLA or authentication issue.
