//! Which ports a sandbox may use.
//!
//! The network switch is coarse: on or off. This adds the case in between —
//! **no general network, but these specific ports are open** — which is what
//! running an app inside a sandbox actually needs: serve on 8000, reach it from
//! a browser, and reach nothing else.
//!
//! Two directions, because they are different permissions:
//!
//! * `listen` — the sandbox may **bind** the port, and you can reach it at
//!   `127.0.0.1:<port>` from this machine. This is the "run my app in a
//!   sandbox" case.
//! * `connect` — the sandbox may make **outbound** connections to that remote
//!   port and no other. `connect: [443]` is "may talk HTTPS, nothing else".
//!
//! # What each backend can actually enforce
//!
//! This differs enough that hiding it would be dishonest, so `note_for` returns
//! the difference and the UI and MCP both show it.
//!
//! | Backend | `listen` | `connect` |
//! |---|---|---|
//! | macOS Seatbelt | exact, per port | exact, per port |
//! | Docker | published to `127.0.0.1`, exact | **not filtered** — see below |
//! | Linux bubblewrap | works, but costs the network namespace | **not filtered** |
//!
//! Seatbelt is the precise one: its profile language filters both directions by
//! port, verified on a real machine before this module was written.
//!
//! Docker and bubblewrap cannot filter outbound by port without adding a
//! firewall or a proxy inside the sandbox. Worse, on both of them **opening a
//! listening port costs the network isolation**: a container with
//! `--network none` cannot publish anything, and a bubblewrap sandbox with
//! `--unshare-net` has no route to the host. So on those two, asking for a
//! listening port grants a network — and that is reported rather than quietly
//! done.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// Ports below this need privileges to bind and are refused.
const MIN_PORT: u16 = 1024;
/// Keeping the list short keeps a sandbox profile readable and bounded.
const MAX_RULES: usize = 16;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PortPolicy {
    /// Ports the sandbox may bind; reachable from this machine.
    #[serde(default)]
    pub listen: Vec<u16>,
    /// Remote ports the sandbox may connect out to.
    #[serde(default)]
    pub connect: Vec<u16>,
}

impl PortPolicy {
    pub fn is_empty(&self) -> bool {
        self.listen.is_empty() && self.connect.is_empty()
    }

    /// True when the policy needs a network to work at all — which is what
    /// forces docker and bubblewrap to give one up.
    pub fn wants_network(&self) -> bool {
        !self.is_empty()
    }
}

/// Clean and check a policy. Rejects rather than silently repairs, because a
/// port the user thought they opened but did not is the kind of thing found
/// much later, by confusion.
pub fn validate(listen: &[u16], connect: &[u16]) -> Result<PortPolicy> {
    // `privileged` applies to the listen list only. Binding a port below 1024
    // needs root; **connecting out** to one is ordinary traffic — 443 is HTTPS,
    // and refusing it would reject the single most useful rule anyone writes.
    let clean = |ps: &[u16], what: &str, privileged: bool| -> Result<Vec<u16>> {
        let mut out: Vec<u16> = Vec::new();
        for p in ps {
            if *p == 0 {
                return Err(anyhow!("port 0 is not a port ({what})"));
            }
            if privileged && *p < MIN_PORT {
                return Err(anyhow!(
                    "port {p} is below {MIN_PORT} ({what}); privileged ports need root and are not allowed here"
                ));
            }
            if !out.contains(p) {
                out.push(*p);
            }
        }
        if out.len() > MAX_RULES {
            return Err(anyhow!("too many {what} ports: {} (max {MAX_RULES})", out.len()));
        }
        out.sort_unstable();
        Ok(out)
    };
    Ok(PortPolicy {
        listen: clean(listen, "listen", true)?,
        connect: clean(connect, "connect", false)?,
    })
}

/// Seatbelt rules for the network section.
///
/// Emitted after a `(deny network*)`, so each `allow` re-opens exactly one
/// port; last matching rule wins in Seatbelt. Verified on macOS: with only
/// `*:53` allowed outbound, a connect to `:443` is refused, and a bind to a
/// port that was not listed is refused too.
pub fn seatbelt_rules(policy: &PortPolicy, network: bool) -> String {
    let mut s = String::new();
    if network {
        // The coarse switch wins: full outbound, plus whatever may listen.
        s.push_str(";; network: enabled at sandbox level\n");
    } else if policy.is_empty() {
        s.push_str(";; ── network fully denied ──\n(deny network*)\n");
        return s;
    } else {
        s.push_str(";; ── network denied except the ports opened below ──\n(deny network*)\n");
    }

    for p in &policy.connect {
        s.push_str(&format!(
            "(allow network-outbound (remote ip \"*:{p}\"))\n"
        ));
    }
    for p in &policy.listen {
        // Binding alone is not enough to be reachable — the inbound connection
        // has to be permitted too, or the server listens and every client is
        // refused.
        s.push_str(&format!("(allow network-bind (local ip \"*:{p}\"))\n"));
        s.push_str(&format!("(allow network-inbound (local ip \"*:{p}\"))\n"));
    }
    s
}

/// `docker run` arguments publishing the listening ports.
///
/// Bound to `127.0.0.1` on purpose: `-p 8000:8000` alone listens on every
/// interface, which would put a sandboxed app on the LAN — the same mistake
/// this repo already made once by binding a Space App to `0.0.0.0`.
pub fn docker_publish_args(policy: &PortPolicy) -> Vec<String> {
    let mut a = Vec::new();
    for p in &policy.listen {
        a.push("-p".into());
        a.push(format!("127.0.0.1:{p}:{p}"));
    }
    a
}

/// What this backend will really do, in the user's terms. `None` when the
/// policy is empty or fully enforceable.
pub fn note_for(backend: &str, isolation: &str, policy: &PortPolicy) -> Option<String> {
    if policy.is_empty() {
        return None;
    }
    match (backend, isolation) {
        ("direct", "seatbelt") => None, // exact, nothing to warn about
        ("docker", _) => Some(
            "On docker, opening a port gives the container a network: published ports need one, \
             so outbound traffic is NOT limited to the `connect` list."
                .into(),
        ),
        ("direct", "bubblewrap") => Some(
            "On Linux, opening a listening port means the sandbox shares this machine's network \
             namespace, so outbound traffic is NOT limited to the `connect` list."
                .into(),
        ),
        _ => Some(
            "This backend cannot enforce per-port rules; the ports are advisory only.".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privileged_ports_are_refused_with_the_reason() {
        let e = validate(&[80], &[]).unwrap_err().to_string();
        assert!(e.contains("below 1024"), "got: {e}");
        assert!(validate(&[], &[443]).is_ok(), "connecting OUT to 443 is fine");
        assert!(validate(&[443], &[]).is_err(), "binding 443 is not");
        assert!(validate(&[], &[0]).is_err(), "port 0 is never a port");
    }

    #[test]
    fn duplicates_collapse_and_the_list_is_bounded() {
        let p = validate(&[8000, 8000, 3000], &[]).unwrap();
        assert_eq!(p.listen, vec![3000, 8000]);
        let many: Vec<u16> = (2000..2000 + MAX_RULES as u16 + 1).collect();
        assert!(validate(&many, &[]).is_err());
    }

    #[test]
    fn an_empty_policy_denies_the_network_outright() {
        let s = seatbelt_rules(&PortPolicy::default(), false);
        assert!(s.contains("(deny network*)"));
        assert!(!s.contains("allow network"));
    }

    #[test]
    fn opened_ports_are_re_allowed_after_the_deny() {
        let p = validate(&[8000], &[443]).unwrap();
        let s = seatbelt_rules(&p, false);
        let deny = s.find("(deny network*)").expect("must deny first");
        let out = s.find(r#"(allow network-outbound (remote ip "*:443"))"#).unwrap();
        let bind = s.find(r#"(allow network-bind (local ip "*:8000"))"#).unwrap();
        assert!(deny < out && deny < bind, "allows must follow the deny to win");
    }

    #[test]
    fn a_listening_port_also_allows_the_inbound_connection() {
        // Bind alone leaves a server that accepts nothing.
        let s = seatbelt_rules(&validate(&[8000], &[]).unwrap(), false);
        assert!(s.contains(r#"(allow network-inbound (local ip "*:8000"))"#));
    }

    #[test]
    fn the_coarse_switch_wins_over_connect_rules() {
        let s = seatbelt_rules(&validate(&[], &[443]).unwrap(), true);
        assert!(!s.contains("(deny network*)"), "network:true must not be narrowed here");
    }

    #[test]
    fn published_ports_stay_on_loopback() {
        let a = docker_publish_args(&validate(&[8000, 9000], &[]).unwrap()).join(" ");
        assert!(a.contains("-p 127.0.0.1:8000:8000"));
        assert!(a.contains("-p 127.0.0.1:9000:9000"));
        assert!(
            !a.contains("-p 8000:8000"),
            "an unqualified publish would expose a sandboxed app to the LAN"
        );
    }

    #[test]
    fn connect_ports_are_not_published() {
        // Outbound permission is not a reason to open a listening socket.
        assert!(docker_publish_args(&validate(&[], &[443]).unwrap()).is_empty());
    }

    #[test]
    fn backends_that_cannot_enforce_say_so() {
        let p = validate(&[8000], &[443]).unwrap();
        assert!(note_for("direct", "seatbelt", &p).is_none(), "seatbelt is exact");
        assert!(note_for("docker", "container", &p).unwrap().contains("NOT limited"));
        assert!(note_for("direct", "bubblewrap", &p).unwrap().contains("NOT limited"));
        assert!(note_for("docker", "container", &PortPolicy::default()).is_none());
    }
}
