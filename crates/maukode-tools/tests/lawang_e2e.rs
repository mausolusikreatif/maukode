//! maukode's real tools against a real `lawangd`, at every isolation level the machine can provide.
//!
//! Skipped unless `LAWANGD_BIN` names a lawangd binary (lawang is a separate project, so CI does
//! not have one). A VM also needs KVM and `LAWANG_TEST_ROOTFS`, an unpacked image directory with
//! `/bin/sh`; `LAWANG_STATIC_AGENT` is passed through to the daemon for image cells.
//!
//!     LAWANGD_BIN=~/lawang/target/debug/lawangd cargo test -p maukode-tools --test lawang_e2e

use maukode_tools::{
    cell_id_for, create_registry_with_backend, destroy_cell, CellIsolation, LawangBackend,
    LawangSettings, PathJail, ToolError, ToolRegistry,
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

const TOKEN: &str = "e2e-token";

struct Daemon {
    child: Child,
    url: String,
    dir: PathBuf,
}

impl Daemon {
    fn start(bin: &str) -> Daemon {
        // Tests run in parallel and each daemon owns its state directory exclusively.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("maukode-e2e-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(bin)
            .args([
                "--state",
                dir.to_str().unwrap(),
                "--listen",
                "tcp:127.0.0.1:0",
                "--token",
                TOKEN,
            ])
            .env("XDG_RUNTIME_DIR", dir.join("xdg"))
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start lawangd");
        let mut line = String::new();
        BufReader::new(child.stderr.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let addr = line.trim().rsplit("tcp:").next().unwrap().to_string();
        Daemon {
            child,
            url: format!("http://{addr}"),
            dir,
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // SAFETY: signalling our own child so it can stop its cells.
        unsafe { libc_kill(self.child.id() as i32, 15) };
        for _ in 0..100 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

async fn available(url: &str, runtime: &str) -> bool {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let body: Value = client
        .get(format!("{url}/v1/capabilities"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    body["runtimes"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|r| r["id"] == runtime && r["status"] == "available")
}

async fn call(registry: &ToolRegistry, tool: &str, args: Value) -> Result<String, ToolError> {
    registry
        .get(tool)
        .unwrap_or_else(|| panic!("no tool {tool}"))
        .execute(args)
        .await
}

async fn bash(registry: &ToolRegistry, command: &str) -> String {
    call(registry, "bash", json!({ "command": command }))
        .await
        .unwrap()
}

/// What an agent does in a project, through the real registry: the same tools, the same
/// arguments. Every step crosses the host/cell boundary in one direction or the other.
async fn scenario(url: &str, isolation: CellIsolation, image: Option<String>) {
    let label = isolation.as_str();
    let project =
        std::env::temp_dir().join(format!("maukode-e2e-proj-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("seed.txt"), "seed text\n").unwrap();
    std::fs::write(project.join("src/lib.rs"), "fn answer() -> u32 { 41 }\n").unwrap();

    let settings = LawangSettings {
        url: url.to_string(),
        token: Some(TOKEN.into()),
        isolation,
        image,
        memory_mb: None,
    };
    let cell = cell_id_for(&format!("e2e-{label}-{}", std::process::id()));
    let backend = Arc::new(LawangBackend::new(settings.clone(), cell.clone(), &project));
    let registry = create_registry_with_backend(&project, PathJail::new(&project), backend);

    // The command sees the project.
    assert!(
        bash(&registry, "cat seed.txt").await.contains("seed text"),
        "{label}"
    );
    assert!(
        bash(&registry, "ls src").await.contains("lib.rs"),
        "{label}: the working directory is the project"
    );

    // A command writes a file; the host's file tools see it (a VM must copy it home first).
    bash(&registry, "echo made-by-the-cell > made.txt").await;
    let read = call(&registry, "read_file", json!({ "path": "made.txt" }))
        .await
        .unwrap();
    assert!(
        read.contains("made-by-the-cell"),
        "{label}: read_file after bash: {read}"
    );
    let found = call(&registry, "grep", json!({ "query": "made-by-the-cell" }))
        .await
        .unwrap();
    assert!(
        found.contains("made.txt"),
        "{label}: grep after bash: {found}"
    );

    // A host tool writes a file; the command sees it (a VM must be given the file).
    call(
        &registry,
        "write_file",
        json!({ "path": "written.txt", "content": "made-by-the-host\n" }),
    )
    .await
    .unwrap();
    assert!(
        bash(&registry, "cat written.txt")
            .await
            .contains("made-by-the-host"),
        "{label}: bash after write_file"
    );

    // An edit made on the host reaches the command, and builds on what the command changed.
    bash(&registry, "echo 'fn extra() {}' >> src/lib.rs").await;
    call(
        &registry,
        "edit_file",
        json!({ "path": "src/lib.rs", "old_content": "41", "new_content": "42" }),
    )
    .await
    .unwrap();
    let lib = bash(&registry, "cat src/lib.rs").await;
    assert!(
        lib.contains("42") && lib.contains("fn extra"),
        "{label}: both edits must survive: {lib}"
    );

    // A file made outside maukode (an editor, git) is never lost to a sync.
    std::fs::write(project.join("made-in-editor.txt"), "precious").unwrap();
    bash(&registry, "echo more >> made.txt").await;
    let _ = call(&registry, "read_file", json!({ "path": "made.txt" }))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(project.join("made-in-editor.txt")).unwrap(),
        "precious",
        "{label}: a file only the host knew about must survive"
    );

    // A command's exit code and stderr reach the model the same way as for a local command.
    let failing = bash(&registry, "echo oops >&2; exit 3").await;
    assert!(
        failing.contains("[stderr]") && failing.contains("oops"),
        "{label}: {failing}"
    );
    assert!(
        failing.contains("[Process exited with code 3]"),
        "{label}: {failing}"
    );

    // A long command is stopped at its timeout, with the message the bash tool always used.
    let slow = call(
        &registry,
        "bash",
        json!({ "command": "sleep 30", "timeout_seconds": 1 }),
    )
    .await;
    let e = slow.unwrap_err().to_string();
    assert!(e.contains("timed out after 1 seconds"), "{label}: {e}");

    // The cell has no network.
    let net = bash(
        &registry,
        "cat /proc/net/dev | tail -n +3 | cut -d: -f1 | tr -d ' ' | tr '\\n' ' '",
    )
    .await;
    assert!(
        net.trim() == "lo",
        "{label}: only loopback expected, saw `{net}`"
    );

    destroy_cell(&settings, &cell).await;
    let _ = std::fs::remove_dir_all(&project);
    eprintln!("scenario {label}: passed");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tools_behave_the_same_at_every_isolation_level_this_machine_has() {
    let Some(bin) = std::env::var("LAWANGD_BIN")
        .ok()
        .filter(|b| Path::new(b).exists())
    else {
        eprintln!("skipped: set LAWANGD_BIN to a lawangd binary");
        return;
    };
    let daemon = Daemon::start(&bin);
    let rootfs = std::env::var("LAWANG_TEST_ROOTFS")
        .ok()
        .filter(|p| Path::new(p).join("bin").exists());
    let mut ran = 0;
    for (isolation, runtime) in [
        (CellIsolation::Process, "bwrap"),
        (CellIsolation::Sandbox, "gvisor"),
        (CellIsolation::Vm, "firecracker"),
    ] {
        if !available(&daemon.url, runtime).await {
            eprintln!(
                "skipped {}: {runtime} is not available here",
                isolation.as_str()
            );
            continue;
        }
        if isolation == CellIsolation::Vm && rootfs.is_none() {
            eprintln!("skipped vm: set LAWANG_TEST_ROOTFS");
            continue;
        }
        let image = (isolation == CellIsolation::Vm)
            .then(|| rootfs.clone())
            .flatten();
        scenario(&daemon.url, isolation, image).await;
        ran += 1;
    }
    assert!(
        ran > 0,
        "no isolation level was available, so nothing was tested"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_token_stops_the_command_with_an_explanation_and_runs_nothing() {
    let Some(bin) = std::env::var("LAWANGD_BIN")
        .ok()
        .filter(|b| Path::new(b).exists())
    else {
        eprintln!("skipped: set LAWANGD_BIN to a lawangd binary");
        return;
    };
    let daemon = Daemon::start(&bin);
    let project = std::env::temp_dir().join(format!("maukode-e2e-badtoken-{}", std::process::id()));
    std::fs::create_dir_all(&project).unwrap();
    let marker = project.join("must-not-exist");
    let settings = LawangSettings {
        url: daemon.url.clone(),
        token: Some("wrong".into()),
        ..LawangSettings::default()
    };
    let backend = Arc::new(LawangBackend::new(settings, "mk-badtoken", &project));
    let registry = create_registry_with_backend(&project, PathJail::new(&project), backend);
    let e = call(
        &registry,
        "bash",
        json!({ "command": format!("touch {}", marker.display()) }),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(e.contains("rejected the token"), "{e}");
    assert!(
        !marker.exists(),
        "a command that was refused must not run anywhere, least of all on the host"
    );
    let _ = std::fs::remove_dir_all(&project);
}
