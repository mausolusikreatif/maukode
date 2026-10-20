//! Where the `bash` tool's commands actually run, and how the host's view of the project stays in
//! step with that place.
//!
//! * [`HostBackend`] runs the command as a local process, exactly as the bash tool always did.
//! * [`LawangBackend`] runs it in a cell of a `lawangd` daemon (bubblewrap, gVisor or a Firecracker
//!   VM) over HTTP. maukode and lawang are separate programs: this speaks lawang's public API.
//!
//! The file tools keep working on the project directory on the host. For a cell that mounts that
//! directory (process and sandbox isolation) that is the same directory the command sees. A VM
//! works on a copy, so [`ExecBackend::before_workspace_access`] brings the VM's changes home before
//! the host reads or writes, and [`ExecBackend::after_host_write`] pushes a file the host wrote into
//! the VM. [`SyncedTool`] adds those two calls around any tool.
//!
//! A backend never falls back to another one: if the lawang daemon cannot be reached the command
//! fails, because quietly running it on the host would defeat the point of choosing a sandbox.

use crate::security::{sandboxed_command, SandboxMode};
use crate::tool::{Tool, ToolError, ToolSource};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where a lawang cell sees the project.
pub const GUEST_WORKSPACE: &str = "/work";

#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub command: String,
    /// A directory inside the project, as a host path.
    pub cwd: PathBuf,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// The text a model sees for a finished command: stdout, then stderr under a marker, then the exit
/// code when it is not zero.
pub fn format_output(out: &ExecOutput) -> String {
    let mut result = String::new();
    if !out.stdout.is_empty() {
        result.push_str(&out.stdout);
    }
    if !out.stderr.is_empty() {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str("[stderr]\n");
        result.push_str(&out.stderr);
    }
    if out.exit_code != 0 {
        result.push_str(&format!("\n[Process exited with code {}]", out.exit_code));
    }
    if result.trim().is_empty() {
        result.push_str("(Command finished with no output)");
    }
    result
}

fn timed_out(timeout: Duration) -> ToolError {
    ToolError::Execution(format!(
        "Command timed out after {} seconds",
        timeout.as_secs()
    ))
}

#[async_trait]
pub trait ExecBackend: Send + Sync {
    /// What the UI shows: `host`, `process`, `sandbox` or `vm`.
    fn label(&self) -> &'static str;

    async fn exec(&self, req: ExecRequest) -> Result<ExecOutput, ToolError>;

    /// Called before a tool reads or writes the project on the host.
    async fn before_workspace_access(&self) -> Result<(), ToolError> {
        Ok(())
    }

    /// Called after a tool wrote `path` on the host.
    async fn after_host_write(&self, _path: &Path) -> Result<(), ToolError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Host
// ---------------------------------------------------------------------------------------------

/// Runs commands as local processes, optionally inside the existing bubblewrap wrapper.
pub struct HostBackend {
    pub sandbox: SandboxMode,
}

impl HostBackend {
    pub fn new(sandbox: SandboxMode) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl ExecBackend for HostBackend {
    fn label(&self) -> &'static str {
        "host"
    }

    async fn exec(&self, req: ExecRequest) -> Result<ExecOutput, ToolError> {
        let mut command = sandboxed_command(
            "bash",
            &["-c".into(), req.command.clone()],
            &req.cwd,
            self.sandbox,
        )
        .map_err(ToolError::Execution)?;
        let child = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::Execution(format!("Failed to spawn bash process: {e}")))?;

        let run = async {
            let output = child
                .wait_with_output()
                .await
                .map_err(|e| ToolError::Execution(format!("Command execution failed: {e}")))?;
            Ok(ExecOutput {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code().unwrap_or(-1),
            })
        };
        tokio::time::timeout(req.timeout, run)
            .await
            .map_err(|_| timed_out(req.timeout))?
    }
}

// ---------------------------------------------------------------------------------------------
// Lawang
// ---------------------------------------------------------------------------------------------

/// How strongly a lawang cell is isolated. The names are lawang's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CellIsolation {
    /// Namespaces on the host kernel (bubblewrap).
    #[default]
    Process,
    /// A user-space kernel (gVisor).
    Sandbox,
    /// A hardware virtual machine with its own kernel (Firecracker). Needs an image.
    Vm,
}

impl CellIsolation {
    pub fn as_str(self) -> &'static str {
        match self {
            CellIsolation::Process => "process",
            CellIsolation::Sandbox => "sandbox",
            CellIsolation::Vm => "vm",
        }
    }
}

fn default_lawang_url() -> String {
    "http://127.0.0.1:7777".to_string()
}

/// Connection and cell settings for [`LawangBackend`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LawangSettings {
    #[serde(default = "default_lawang_url")]
    pub url: String,
    /// Bearer token of the daemon, when it was started with one.
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub isolation: CellIsolation,
    /// An image such as `alpine:3.20` for the cell's root filesystem. Required for `vm`.
    #[serde(default)]
    pub image: Option<String>,
    /// Memory limit of the cell in MiB (for a VM, its memory).
    #[serde(default)]
    pub memory_mb: Option<u64>,
}

impl Default for LawangSettings {
    fn default() -> Self {
        Self {
            url: default_lawang_url(),
            token: None,
            isolation: CellIsolation::default(),
            image: None,
            memory_mb: None,
        }
    }
}

// The token must not end up in logs or panic messages.
impl fmt::Debug for LawangSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LawangSettings")
            .field("url", &self.url)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("isolation", &self.isolation)
            .field("image", &self.image)
            .field("memory_mb", &self.memory_mb)
            .finish()
    }
}

/// A cell id that lawang accepts (`[a-z0-9-]`, at most 63 characters) for a session.
pub fn cell_id_for(session: &str) -> String {
    let clean: String = session
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .take(24)
        .collect();
    format!("mk-{clean}")
}

/// `sh -c SCRIPT sh COMMAND`: prefer bash when the image has it, fall back to sh (Alpine has none).
const SHELL_SCRIPT: &str =
    "if command -v bash >/dev/null 2>&1; then exec bash -c \"$1\"; else exec sh -c \"$1\"; fi";

/// An archive holding one regular file, for pushing a file into a VM.
pub fn single_file_tar(name: &str, mode: u32, data: &[u8]) -> Vec<u8> {
    let mut header = [0u8; 512];
    let put = |header: &mut [u8; 512], at: usize, text: &str| {
        header[at..at + text.len()].copy_from_slice(text.as_bytes());
    };
    let name = name.as_bytes();
    let n = name.len().min(99);
    header[..n].copy_from_slice(&name[..n]);
    put(&mut header, 100, &format!("{:07o}\0", mode & 0o7777));
    put(&mut header, 108, "0000000\0");
    put(&mut header, 116, "0000000\0");
    put(&mut header, 124, &format!("{:011o}\0", data.len()));
    put(&mut header, 136, "00000000000\0");
    header[156] = b'0';
    put(&mut header, 257, "ustar\0");
    put(&mut header, 263, "00");
    // The checksum is computed with its own field read as spaces.
    header[148..156].fill(b' ');
    let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    put(&mut header, 148, &format!("{sum:06o}\0 "));
    let mut out = header.to_vec();
    out.extend_from_slice(data);
    out.resize(out.len() + (512 - data.len() % 512) % 512, 0);
    out.extend_from_slice(&[0u8; 1024]);
    out
}

#[derive(Default)]
struct LawangState {
    /// The cell exists and matches the settings.
    ready: bool,
    /// The VM may hold changes the host directory does not have yet.
    dirty: bool,
}

pub struct LawangBackend {
    settings: LawangSettings,
    cell: String,
    workspace: PathBuf,
    http: reqwest::Client,
    state: tokio::sync::Mutex<LawangState>,
}

impl LawangBackend {
    pub fn new(
        settings: LawangSettings,
        cell: impl Into<String>,
        workspace: impl Into<PathBuf>,
    ) -> Self {
        let http = reqwest::Client::builder()
            // The daemon is usually local; an environment proxy must not intercept it.
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Self {
            settings,
            cell: cell.into(),
            workspace: workspace.into(),
            http,
            state: tokio::sync::Mutex::new(LawangState::default()),
        }
    }

    pub fn settings(&self) -> &LawangSettings {
        &self.settings
    }

    pub fn cell_id(&self) -> &str {
        &self.cell
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{path}", self.settings.url.trim_end_matches('/'));
        let rb = self.http.request(method, url);
        match &self.settings.token {
            Some(t) if !t.is_empty() => rb.bearer_auth(t),
            _ => rb,
        }
    }

    fn unreachable(&self, e: &reqwest::Error) -> ToolError {
        ToolError::Execution(format!(
            "cannot reach the lawang daemon at {}: {e}. Start it (`lawangd --listen tcp:ADDR --token TOKEN`) \
             or switch the execution backend back to Host in Settings; the command was not run.",
            self.settings.url
        ))
    }

    /// Send a request and return the status with the JSON body (`Null` when there is none).
    async fn send(&self, rb: reqwest::RequestBuilder) -> Result<(u16, Value), ToolError> {
        let resp = rb.send().await.map_err(|e| self.unreachable(&e))?;
        let status = resp.status().as_u16();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ToolError::Execution(format!("lawang: reading the reply failed: {e}")))?;
        Ok((
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        ))
    }

    fn api_error(what: &str, status: u16, body: &Value) -> ToolError {
        let msg = body["error"].as_str().unwrap_or("no details");
        match status {
            401 => ToolError::Execution(format!(
                "lawang: {what}: the daemon rejected the token (check the token in Settings)"
            )),
            _ => ToolError::Execution(format!("lawang: {what} failed ({status}): {msg}")),
        }
    }

    fn create_body(&self) -> Value {
        let mut spec = json!({
            "workspace": {
                "host": self.workspace,
                "guest": GUEST_WORKSPACE,
                "writable": true,
            },
        });
        if let Some(mb) = self.settings.memory_mb {
            spec["limits"] = json!({ "memory_bytes": mb.saturating_mul(1 << 20) });
        }
        let mut body = json!({
            "id": self.cell,
            "isolation": self.settings.isolation.as_str(),
            "spec": spec,
        });
        if let Some(image) = self.image() {
            // An absolute path is an image that was unpacked earlier; anything else is a name to
            // pull (`alpine:3.20`).
            if Path::new(image).is_absolute() {
                body["spec"]["rootfs"] = json!(image);
            } else {
                body["image"] = json!(image);
            }
        }
        body
    }

    /// The configured image, trimmed, when there is one.
    fn image(&self) -> Option<&str> {
        self.settings
            .image
            .as_deref()
            .map(str::trim)
            .filter(|i| !i.is_empty())
    }

    /// Does an existing cell match what the settings ask for? A different isolation, a root
    /// filesystem where there was none (or the reverse), or another memory limit means it must be
    /// replaced. The project itself lives on the host, so replacing a cell loses nothing there.
    fn matches(&self, existing: &Value) -> bool {
        let wants_image = self.image().is_some();
        let has_image = !existing["spec"]["rootfs"].is_null();
        // A path can be compared exactly; a name only tells us that a root filesystem is wanted.
        let same_path = match self.image().filter(|i| Path::new(i).is_absolute()) {
            Some(path) => existing["spec"]["rootfs"] == path,
            None => true,
        };
        let wanted_memory = self.settings.memory_mb.map(|mb| mb.saturating_mul(1 << 20));
        existing["isolation"] == self.settings.isolation.as_str()
            && wants_image == has_image
            && same_path
            && existing["spec"]["limits"]["memory_bytes"].as_u64() == wanted_memory
    }

    async fn ensure_cell(&self) -> Result<(), ToolError> {
        let mut st = self.state.lock().await;
        if st.ready {
            return Ok(());
        }
        let path = format!("/v1/cells/{}", self.cell);
        let (status, existing) = self.send(self.request(reqwest::Method::GET, &path)).await?;
        match status {
            200 if self.matches(&existing) => {}
            200 => {
                let (s, b) = self
                    .send(self.request(reqwest::Method::DELETE, &path))
                    .await?;
                if s != 204 && s != 404 {
                    return Err(Self::api_error("replacing the cell", s, &b));
                }
                self.create().await?;
            }
            404 => self.create().await?,
            s => return Err(Self::api_error("looking up the cell", s, &existing)),
        }
        st.ready = true;
        st.dirty = false;
        Ok(())
    }

    async fn create(&self) -> Result<(), ToolError> {
        // Creating with an image may download it, which can take minutes.
        let rb = self
            .request(reqwest::Method::POST, "/v1/cells")
            .timeout(Duration::from_secs(900))
            .json(&self.create_body());
        let (status, body) = self.send(rb).await?;
        match status {
            // Someone else created it between our look and our create: fine.
            201 | 409 => Ok(()),
            s => Err(Self::api_error("creating the cell", s, &body)),
        }
    }

    /// The path of a host path inside the cell, or `None` when it is outside the project.
    fn guest_path(&self, host: &Path) -> Option<String> {
        let rel = host.strip_prefix(&self.workspace).ok()?;
        if rel.as_os_str().is_empty() {
            return Some(GUEST_WORKSPACE.to_string());
        }
        let mut guest = PathBuf::from(GUEST_WORKSPACE);
        guest.push(rel);
        guest.to_str().map(str::to_string)
    }

    fn is_vm(&self) -> bool {
        self.settings.isolation == CellIsolation::Vm
    }

    async fn forget_cell(&self) {
        self.state.lock().await.ready = false;
    }
}

#[async_trait]
impl ExecBackend for LawangBackend {
    fn label(&self) -> &'static str {
        self.settings.isolation.as_str()
    }

    async fn exec(&self, req: ExecRequest) -> Result<ExecOutput, ToolError> {
        let cwd = self.guest_path(&req.cwd).ok_or_else(|| {
            ToolError::Execution(format!(
                "{} is outside the project, which a lawang cell cannot see; run the command from inside {}",
                req.cwd.display(),
                self.workspace.display()
            ))
        })?;
        let body = json!({
            "cmd": ["sh", "-c", SHELL_SCRIPT, "sh", req.command],
            "cwd": cwd,
            "timeout_ms": u64::try_from(req.timeout.as_millis()).unwrap_or(u64::MAX),
            "collect": true,
        });

        // One retry: the cell may have been removed behind our back (daemon restarted with an
        // empty state, someone deleted it).
        for attempt in 0..2 {
            self.ensure_cell().await?;
            let started = Instant::now();
            let rb = self
                .request(
                    reqwest::Method::POST,
                    &format!("/v1/cells/{}/exec", self.cell),
                )
                .timeout(req.timeout + Duration::from_secs(60))
                .json(&body);
            let (status, reply) = self.send(rb).await?;
            if status == 404 && attempt == 0 {
                self.forget_cell().await;
                continue;
            }
            if status != 200 {
                return Err(Self::api_error("running the command", status, &reply));
            }
            if self.is_vm() {
                self.state.lock().await.dirty = true;
            }
            let exit_code = reply["exit_code"].as_i64().unwrap_or(-1) as i32;
            // The agent ends a command that outlives its timeout with SIGKILL.
            if exit_code == -9 && started.elapsed() + Duration::from_secs(2) >= req.timeout {
                return Err(timed_out(req.timeout));
            }
            return Ok(ExecOutput {
                stdout: reply["stdout"].as_str().unwrap_or_default().to_string(),
                stderr: reply["stderr"].as_str().unwrap_or_default().to_string(),
                exit_code,
            });
        }
        Err(ToolError::Execution(
            "lawang: the cell disappeared while running the command".into(),
        ))
    }

    async fn before_workspace_access(&self) -> Result<(), ToolError> {
        if !self.is_vm() {
            // The cell mounts the project: the host directory is always current.
            return Ok(());
        }
        let mut st = self.state.lock().await;
        if !st.ready || !st.dirty {
            return Ok(());
        }
        let (status, body) = self
            .send(self.request(
                reqwest::Method::POST,
                &format!("/v1/cells/{}/flush", self.cell),
            ))
            .await?;
        if status != 204 {
            return Err(Self::api_error(
                "copying the VM's changes to the project",
                status,
                &body,
            ));
        }
        st.dirty = false;
        Ok(())
    }

    async fn after_host_write(&self, path: &Path) -> Result<(), ToolError> {
        if !self.is_vm() {
            return Ok(());
        }
        let st = self.state.lock().await;
        if !st.ready {
            return Ok(());
        }
        drop(st);
        // Only push into a VM that is running: a stopped one uploads the project when it starts,
        // and the upload endpoint would wake it.
        let (status, cell) = self
            .send(self.request(reqwest::Method::GET, &format!("/v1/cells/{}", self.cell)))
            .await?;
        if status != 200 || cell["state"] != "running" {
            return Ok(());
        }
        let (Some(guest), Some(name)) = (
            self.guest_path(path),
            path.file_name().and_then(|n| n.to_str()),
        ) else {
            return Ok(());
        };
        let dir = Path::new(&guest)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or(GUEST_WORKSPACE)
            .to_string();
        let data = tokio::fs::read(path).await.map_err(|e| {
            ToolError::Execution(format!(
                "cannot read {} to copy it into the VM: {e}",
                path.display()
            ))
        })?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::metadata(path)
                .await
                .map(|m| m.permissions().mode())
                .unwrap_or(0o644)
        };
        #[cfg(not(unix))]
        let mode = 0o644;
        let rb = self
            .request(
                reqwest::Method::PUT,
                &format!("/v1/cells/{}/files", self.cell),
            )
            .query(&[("path", dir.as_str())])
            .body(single_file_tar(name, mode, &data));
        let (status, body) = self.send(rb).await?;
        if status != 204 {
            return Err(Self::api_error(
                "copying the file into the VM",
                status,
                &body,
            ));
        }
        Ok(())
    }
}

/// Remove a session's cell from the daemon (best effort: the cell is gone or the daemon is down,
/// either way there is nothing left to clean up for the caller).
pub async fn destroy_cell(settings: &LawangSettings, cell: &str) {
    let backend = LawangBackend::new(settings.clone(), cell, PathBuf::new());
    let _ = backend
        .send(backend.request(reqwest::Method::DELETE, &format!("/v1/cells/{cell}")))
        .await;
}

// ---------------------------------------------------------------------------------------------
// File tools
// ---------------------------------------------------------------------------------------------

/// Wraps a file tool so the backend can keep its own copy of the project in step: before the tool
/// runs, pending changes from the cell are brought to the host; after a successful write, the file
/// is pushed into the cell.
pub struct SyncedTool {
    inner: Arc<dyn Tool>,
    backend: Arc<dyn ExecBackend>,
    jail: crate::security::PathJail,
    writes: bool,
}

impl SyncedTool {
    /// `writes`: the tool changes the file named by its `path` argument, or the files of its
    /// `patch` argument (`apply_patch`).
    pub fn new(
        inner: Arc<dyn Tool>,
        backend: Arc<dyn ExecBackend>,
        jail: crate::security::PathJail,
        writes: bool,
    ) -> Self {
        Self {
            inner,
            backend,
            jail,
            writes,
        }
    }
}

/// Files a writing tool call changes: its `path`, or every file of its `patch`.
fn written_paths(args: &Value) -> Vec<String> {
    if let Some(path) = args["path"].as_str() {
        return vec![path.to_string()];
    }
    args["patch"]
        .as_str()
        .and_then(|patch| crate::patch::parse_patch(patch).ok())
        .map(|ops| crate::patch::touched_paths(&ops))
        .unwrap_or_default()
}

#[async_trait]
impl Tool for SyncedTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
    }

    fn source(&self) -> ToolSource {
        self.inner.source()
    }

    fn token_cost(&self) -> Option<usize> {
        self.inner.token_cost()
    }

    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        self.backend.before_workspace_access().await?;
        let targets = if self.writes {
            written_paths(&args)
        } else {
            Vec::new()
        };
        let from_patch = args.get("patch").is_some();
        let result = self.inner.execute(args).await?;
        for path in targets {
            let Ok(path) = self.jail.validate_path(&path) else {
                continue;
            };
            // A patch may delete files: those have nothing to push.
            if !from_patch || path.exists() {
                self.backend.after_host_write(&path).await?;
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Debug, Clone)]
    struct Seen {
        method: String,
        path: String,
        auth: Option<String>,
        body: Vec<u8>,
    }

    impl Seen {
        fn json(&self) -> Value {
            serde_json::from_slice(&self.body).unwrap_or(Value::Null)
        }
    }

    type Handler = Box<dyn Fn(&Seen, usize) -> (u16, String) + Send + Sync>;

    /// A stand-in for lawangd: records every request and answers with whatever `handler` says.
    /// `handler` also gets how many requests came before, for scripted sequences.
    async fn fake_daemon(handler: Handler) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let handler = Arc::new(handler);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let (log, handler) = (log.clone(), handler.clone());
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 4096];
                    let head_end = loop {
                        let n = stream.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            break p + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    while raw.len() < head_end + length {
                        let n = stream.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                    }
                    let mut first = head.lines().next().unwrap_or("").split(' ');
                    let (method, target) = (
                        first.next().unwrap_or("").to_string(),
                        first.next().unwrap_or("").to_string(),
                    );
                    let auth = head.lines().find_map(|l| {
                        l.to_ascii_lowercase()
                            .starts_with("authorization:")
                            .then(|| {
                                l.split_once(':')
                                    .map(|x| x.1.trim().to_string())
                                    .unwrap_or_default()
                            })
                    });
                    let request = Seen {
                        method,
                        path: target,
                        auth,
                        body: raw[head_end..].to_vec(),
                    };
                    let index = {
                        let mut l = log.lock().unwrap();
                        l.push(request.clone());
                        l.len() - 1
                    };
                    let (status, body) = handler(&request, index);
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        (url, seen)
    }

    fn settings(url: &str, isolation: CellIsolation) -> LawangSettings {
        LawangSettings {
            url: url.to_string(),
            token: Some("tok".into()),
            isolation,
            image: None,
            memory_mb: None,
        }
    }

    fn request(command: &str, cwd: &str, secs: u64) -> ExecRequest {
        ExecRequest {
            command: command.into(),
            cwd: cwd.into(),
            timeout: Duration::from_secs(secs),
        }
    }

    const OK_EXEC: &str = r#"{"stdout":"hi\n","stderr":"","exit_code":0}"#;

    fn routes(cell_status: u16, cell_body: &'static str) -> Handler {
        Box::new(move |r, _| match (r.method.as_str(), r.path.as_str()) {
            ("GET", p) if p.starts_with("/v1/cells/mk-") && !p.contains("/exec") => {
                (cell_status, cell_body.to_string())
            }
            ("POST", "/v1/cells") => (201, "{}".to_string()),
            ("DELETE", _) => (204, String::new()),
            ("POST", p) if p.ends_with("/exec") => (200, OK_EXEC.to_string()),
            ("POST", p) if p.ends_with("/flush") => (204, String::new()),
            ("PUT", p) if p.contains("/files") => (204, String::new()),
            _ => (500, r#"{"error":"unexpected"}"#.to_string()),
        })
    }

    fn paths(seen: &Arc<Mutex<Vec<Seen>>>) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .map(|s| format!("{} {}", s.method, s.path))
            .collect()
    }

    #[tokio::test]
    async fn a_missing_cell_is_created_with_the_project_mounted_and_the_command_runs_in_it() {
        let (url, seen) = fake_daemon(routes(404, r#"{"error":"no such cell"}"#)).await;
        let mut s = settings(&url, CellIsolation::Sandbox);
        s.image = Some(" alpine:3.20 ".into());
        s.memory_mb = Some(256);
        let backend = LawangBackend::new(s, "mk-abc", "/proj");
        let out = backend
            .exec(request("echo hi", "/proj/sub", 5))
            .await
            .unwrap();
        assert_eq!((out.stdout.as_str(), out.exit_code), ("hi\n", 0));

        assert_eq!(
            paths(&seen),
            [
                "GET /v1/cells/mk-abc",
                "POST /v1/cells",
                "POST /v1/cells/mk-abc/exec"
            ]
        );
        let log = seen.lock().unwrap();
        assert!(
            log.iter().all(|r| r.auth.as_deref() == Some("Bearer tok")),
            "every call is authenticated"
        );
        let create = log[1].json();
        assert_eq!(create["id"], "mk-abc");
        assert_eq!(create["isolation"], "sandbox");
        assert_eq!(create["image"], "alpine:3.20", "the image name is trimmed");
        assert_eq!(
            create["spec"]["workspace"],
            json!({"host": "/proj", "guest": "/work", "writable": true})
        );
        assert_eq!(create["spec"]["limits"]["memory_bytes"], 256u64 << 20);
        let exec = log[2].json();
        assert_eq!(
            exec["cwd"], "/work/sub",
            "the host path is translated into the cell"
        );
        assert_eq!(exec["timeout_ms"], 5000);
        assert_eq!(exec["collect"], true);
        assert_eq!(exec["cmd"][0], "sh");
        assert_eq!(
            exec["cmd"][4], "echo hi",
            "the command is passed as an argument, never spliced into a script"
        );
    }

    #[tokio::test]
    async fn an_image_path_is_sent_as_a_root_filesystem_and_a_name_as_an_image_to_pull() {
        for (image, key, other) in [
            ("/images/alpine", "rootfs", "image"),
            ("alpine:3.20", "image", "rootfs"),
        ] {
            let (url, seen) = fake_daemon(routes(404, "{}")).await;
            let mut s = settings(&url, CellIsolation::Vm);
            s.image = Some(image.into());
            LawangBackend::new(s, "mk-abc", "/proj")
                .exec(request("x", "/proj", 5))
                .await
                .unwrap();
            let create = seen
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.method == "POST" && r.path == "/v1/cells")
                .unwrap()
                .json();
            let found = if key == "rootfs" {
                &create["spec"]["rootfs"]
            } else {
                &create["image"]
            };
            assert_eq!(found, image, "{create}");
            let absent = if other == "rootfs" {
                &create["spec"]["rootfs"]
            } else {
                &create["image"]
            };
            assert!(absent.is_null(), "{create}");
        }
    }

    #[tokio::test]
    async fn changing_the_image_path_replaces_the_cell() {
        let (url, seen) = fake_daemon(routes(
            200,
            r#"{"isolation":"vm","spec":{"rootfs":"/images/old","limits":{}}}"#,
        ))
        .await;
        let mut s = settings(&url, CellIsolation::Vm);
        s.image = Some("/images/new".into());
        LawangBackend::new(s, "mk-abc", "/proj")
            .exec(request("x", "/proj", 5))
            .await
            .unwrap();
        assert!(
            paths(&seen).iter().any(|p| p.starts_with("DELETE")),
            "{:?}",
            paths(&seen)
        );
    }

    #[tokio::test]
    async fn a_matching_cell_is_reused_and_looked_up_only_once() {
        let (url, seen) = fake_daemon(routes(
            200,
            r#"{"isolation":"process","spec":{"limits":{}}}"#,
        ))
        .await;
        let backend = LawangBackend::new(settings(&url, CellIsolation::Process), "mk-abc", "/proj");
        backend.exec(request("a", "/proj", 5)).await.unwrap();
        backend.exec(request("b", "/proj", 5)).await.unwrap();
        let p = paths(&seen);
        assert_eq!(
            p.iter().filter(|x| x.starts_with("GET")).count(),
            1,
            "{p:?}"
        );
        assert!(!p.iter().any(|x| x == "POST /v1/cells"), "{p:?}");
    }

    #[tokio::test]
    async fn a_cell_with_other_settings_is_replaced() {
        let (url, seen) = fake_daemon(routes(
            200,
            r#"{"isolation":"process","spec":{"limits":{}}}"#,
        ))
        .await;
        // The settings now ask for a sandbox, but the existing cell is a plain process cell.
        let backend = LawangBackend::new(settings(&url, CellIsolation::Sandbox), "mk-abc", "/proj");
        backend.exec(request("a", "/proj", 5)).await.unwrap();
        let p = paths(&seen);
        assert_eq!(
            &p[..3],
            [
                "GET /v1/cells/mk-abc",
                "DELETE /v1/cells/mk-abc",
                "POST /v1/cells"
            ],
            "{p:?}"
        );
    }

    #[tokio::test]
    async fn an_image_or_memory_difference_also_replaces_the_cell() {
        let existing =
            r#"{"isolation":"process","spec":{"rootfs":"/x","limits":{"memory_bytes":1048576}}}"#;
        for (image, mb, replaced) in [
            (None, None, true),
            (Some("alpine"), Some(1u64), false),
            (Some("alpine"), Some(2u64), true),
        ] {
            let (url, seen) = fake_daemon(routes(200, existing)).await;
            let mut s = settings(&url, CellIsolation::Process);
            s.image = image.map(String::from);
            s.memory_mb = mb;
            LawangBackend::new(s, "mk-abc", "/proj")
                .exec(request("a", "/proj", 5))
                .await
                .unwrap();
            let deleted = paths(&seen).iter().any(|p| p.starts_with("DELETE"));
            assert_eq!(deleted, replaced, "image={image:?} memory={mb:?}");
        }
    }

    #[tokio::test]
    async fn a_daemon_that_is_down_fails_the_command_instead_of_running_it_elsewhere() {
        let backend = LawangBackend::new(
            settings("http://127.0.0.1:1", CellIsolation::Process),
            "mk-abc",
            "/proj",
        );
        let e = backend
            .exec(request("echo should-not-run", "/proj", 5))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("cannot reach the lawang daemon") && e.contains("was not run"),
            "{e}"
        );
    }

    #[tokio::test]
    async fn a_rejected_token_is_explained() {
        let (url, _) = fake_daemon(Box::new(|_, _| {
            (401, r#"{"error":"missing or wrong bearer token"}"#.into())
        }))
        .await;
        let backend = LawangBackend::new(settings(&url, CellIsolation::Process), "mk-abc", "/proj");
        let e = backend
            .exec(request("x", "/proj", 5))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("rejected the token"), "{e}");
    }

    #[tokio::test]
    async fn the_daemons_own_error_text_reaches_the_user() {
        let handler: Handler = Box::new(|r, _| match (r.method.as_str(), r.path.as_str()) {
            ("GET", _) => (404, "{}".into()),
            _ => (400, r#"{"error":"a vm cell needs an image"}"#.into()),
        });
        let (url, _) = fake_daemon(handler).await;
        let backend = LawangBackend::new(settings(&url, CellIsolation::Vm), "mk-abc", "/proj");
        let e = backend
            .exec(request("x", "/proj", 5))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("creating the cell") && e.contains("needs an image"),
            "{e}"
        );
    }

    #[tokio::test]
    async fn only_a_kill_at_the_deadline_counts_as_a_timeout() {
        let killed = r#"{"stdout":"","stderr":"","exit_code":-9}"#;
        let handler = |body: &'static str| -> Handler {
            Box::new(move |r, _| match (r.method.as_str(), r.path.as_str()) {
                ("GET", _) => (
                    200,
                    r#"{"isolation":"process","spec":{"limits":{}}}"#.into(),
                ),
                _ => (200, body.to_string()),
            })
        };
        // A one-second limit and an immediate SIGKILL: the deadline was reached.
        let (url, _) = fake_daemon(handler(killed)).await;
        let backend = LawangBackend::new(settings(&url, CellIsolation::Process), "mk-abc", "/proj");
        let e = backend
            .exec(request("sleep 9", "/proj", 1))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("timed out after 1 seconds"), "{e}");
        // The same -9 long before a two-minute limit is something else killing it: report the code.
        let backend = LawangBackend::new(settings(&url, CellIsolation::Process), "mk-abc", "/proj");
        let out = backend
            .exec(request("kill -9 $$", "/proj", 120))
            .await
            .unwrap();
        assert_eq!(out.exit_code, -9);
    }

    #[test]
    fn host_paths_map_into_the_cell_without_stray_slashes() {
        let b = LawangBackend::new(LawangSettings::default(), "mk-x", "/proj");
        assert_eq!(b.guest_path(Path::new("/proj")).as_deref(), Some("/work"));
        assert_eq!(
            b.guest_path(Path::new("/proj/a/b")).as_deref(),
            Some("/work/a/b")
        );
        assert_eq!(
            b.guest_path(Path::new("/projects/other")),
            None,
            "a sibling that shares a prefix is outside"
        );
        assert_eq!(b.guest_path(Path::new("/etc")), None);
    }

    #[tokio::test]
    async fn a_directory_outside_the_project_is_refused_before_any_request() {
        let (url, seen) = fake_daemon(routes(200, "{}")).await;
        let backend = LawangBackend::new(settings(&url, CellIsolation::Process), "mk-abc", "/proj");
        let e = backend
            .exec(request("ls", "/etc", 5))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("outside the project"), "{e}");
        assert!(
            seen.lock().unwrap().is_empty(),
            "nothing may reach the daemon"
        );
    }

    #[tokio::test]
    async fn a_cell_that_vanished_is_created_again_and_the_command_retried_once() {
        let handler: Handler = Box::new(|r, _| match (r.method.as_str(), r.path.as_str()) {
            ("GET", _) => (
                200,
                r#"{"isolation":"process","spec":{"limits":{}}}"#.into(),
            ),
            ("POST", p) if p.ends_with("/exec") => {
                // First exec: the daemon has lost the cell. Afterwards it works.
                static CALLS: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                if CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    (404, r#"{"error":"no such cell"}"#.into())
                } else {
                    (200, OK_EXEC.into())
                }
            }
            _ => (201, "{}".into()),
        });
        let (url, seen) = fake_daemon(handler).await;
        let backend = LawangBackend::new(settings(&url, CellIsolation::Process), "mk-abc", "/proj");
        let out = backend.exec(request("echo hi", "/proj", 5)).await.unwrap();
        assert_eq!(out.stdout, "hi\n");
        assert_eq!(
            paths(&seen).iter().filter(|p| p.ends_with("/exec")).count(),
            2
        );
    }

    #[tokio::test]
    async fn a_vm_brings_its_changes_home_once_before_the_host_reads_and_not_otherwise() {
        let (url, seen) = fake_daemon(routes(
            200,
            r#"{"isolation":"vm","state":"running","spec":{"limits":{}}}"#,
        ))
        .await;
        let backend = LawangBackend::new(settings(&url, CellIsolation::Vm), "mk-abc", "/proj");
        // Nothing has run yet: nothing to bring home.
        backend.before_workspace_access().await.unwrap();
        assert!(seen.lock().unwrap().is_empty());
        backend.exec(request("touch x", "/proj", 5)).await.unwrap();
        backend.before_workspace_access().await.unwrap();
        backend.before_workspace_access().await.unwrap();
        let flushes = paths(&seen)
            .iter()
            .filter(|p| p.ends_with("/flush"))
            .count();
        assert_eq!(
            flushes,
            1,
            "one flush covers both reads: {:?}",
            paths(&seen)
        );
        // Another command makes the VM dirty again.
        backend.exec(request("touch y", "/proj", 5)).await.unwrap();
        backend.before_workspace_access().await.unwrap();
        assert_eq!(
            paths(&seen)
                .iter()
                .filter(|p| p.ends_with("/flush"))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn a_file_the_host_wrote_is_pushed_into_a_running_vm_and_skipped_for_a_stopped_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/notes.txt");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "from the host").unwrap();

        for (state, pushed) in [("running", true), ("stopped", false)] {
            let body: &'static str = Box::leak(
                format!(r#"{{"isolation":"vm","state":"{state}","spec":{{"limits":{{}}}}}}"#)
                    .into_boxed_str(),
            );
            let (url, seen) = fake_daemon(routes(200, body)).await;
            let backend =
                LawangBackend::new(settings(&url, CellIsolation::Vm), "mk-abc", dir.path());
            // The cell must be known first (an exec ran, or the settings were checked).
            backend
                .exec(request("true", dir.path().to_str().unwrap(), 5))
                .await
                .unwrap();
            backend.after_host_write(&file).await.unwrap();
            let puts: Vec<Seen> = seen
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.method == "PUT")
                .cloned()
                .collect();
            assert_eq!(!puts.is_empty(), pushed, "state {state}");
            if pushed {
                assert!(
                    puts[0].path.contains("path=%2Fwork%2Fsub")
                        || puts[0].path.contains("path=/work/sub"),
                    "{}",
                    puts[0].path
                );
                let tar = &puts[0].body;
                assert_eq!(&tar[..9], b"notes.txt");
                assert_eq!(&tar[512..525], b"from the host");
            }
        }
    }

    #[tokio::test]
    async fn the_host_hooks_do_nothing_for_cells_that_mount_the_project() {
        let (url, seen) = fake_daemon(routes(
            200,
            r#"{"isolation":"sandbox","spec":{"limits":{}}}"#,
        ))
        .await;
        let backend = LawangBackend::new(settings(&url, CellIsolation::Sandbox), "mk-abc", "/proj");
        backend.exec(request("x", "/proj", 5)).await.unwrap();
        let before = seen.lock().unwrap().len();
        backend.before_workspace_access().await.unwrap();
        backend
            .after_host_write(Path::new("/proj/a.txt"))
            .await
            .unwrap();
        assert_eq!(
            seen.lock().unwrap().len(),
            before,
            "a live mount is always current"
        );
    }

    #[test]
    fn a_single_file_archive_is_well_formed() {
        let data = b"hello world";
        let tar = single_file_tar("a.txt", 0o755, data);
        assert_eq!(tar.len() % 512, 0);
        assert_eq!(
            tar.len(),
            512 + 512 + 1024,
            "header, one padded block, two zero blocks"
        );
        assert_eq!(&tar[..5], b"a.txt");
        assert_eq!(&tar[100..107], b"0000755");
        assert_eq!(&tar[124..135], b"00000000013", "11 bytes in octal");
        assert_eq!(&tar[257..262], b"ustar");
        // The checksum counts the header with its own field as spaces.
        let mut header = tar[..512].to_vec();
        let stored =
            u32::from_str_radix(String::from_utf8_lossy(&header[148..154]).trim(), 8).unwrap();
        header[148..156].fill(b' ');
        assert_eq!(stored, header.iter().map(|b| u32::from(*b)).sum::<u32>());
        assert_eq!(&tar[512..523], data);
        assert!(tar[1024..].iter().all(|b| *b == 0));
    }

    #[test]
    fn cell_ids_are_valid_for_lawang_whatever_the_session_id_looks_like() {
        let id = cell_id_for("7F3A9C2E-1B4D-4E8F-9A6C-0D5B2E1F3A7C");
        assert_eq!(id, "mk-7f3a9c2e1b4d4e8f9a6c0d5b");
        assert!(
            id.len() <= 63
                && id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
        assert_eq!(cell_id_for("../etc"), "mk-etc");
        assert_eq!(cell_id_for("x"), cell_id_for("x"));
    }

    #[test]
    fn settings_default_sensibly_and_never_print_the_token() {
        let s: LawangSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.url, "http://127.0.0.1:7777");
        assert_eq!(s.isolation, CellIsolation::Process);
        assert_eq!((s.token, s.image, s.memory_mb), (None, None, None));
        let shown = format!("{:?}", settings("http://x", CellIsolation::Vm));
        assert!(
            shown.contains("redacted") && !shown.contains("tok\""),
            "{shown}"
        );
    }

    #[test]
    fn command_output_is_formatted_as_the_bash_tool_always_did() {
        let f = |o: &str, e: &str, c: i32| {
            format_output(&ExecOutput {
                stdout: o.into(),
                stderr: e.into(),
                exit_code: c,
            })
        };
        assert_eq!(f("out\n", "", 0), "out\n");
        assert_eq!(f("out", "err\n", 0), "out\n[stderr]\nerr\n");
        assert_eq!(f("", "", 0), "(Command finished with no output)");
        assert_eq!(f("out\n", "", 3), "out\n\n[Process exited with code 3]");
    }

    // --- the file-tool wrapper -----------------------------------------------------------

    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ExecBackend for Recorder {
        fn label(&self) -> &'static str {
            "test"
        }
        async fn exec(&self, _: ExecRequest) -> Result<ExecOutput, ToolError> {
            unreachable!()
        }
        async fn before_workspace_access(&self) -> Result<(), ToolError> {
            self.calls.lock().unwrap().push("before".into());
            Ok(())
        }
        async fn after_host_write(&self, path: &Path) -> Result<(), ToolError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("after {}", path.display()));
            Ok(())
        }
    }

    struct Inner {
        log: Arc<Recorder>,
        fail: bool,
    }

    #[async_trait]
    impl Tool for Inner {
        fn name(&self) -> &str {
            "write_file"
        }
        fn description(&self) -> &str {
            "d"
        }
        fn parameters(&self) -> Value {
            json!({})
        }
        async fn execute(&self, _: Value) -> Result<String, ToolError> {
            self.log.calls.lock().unwrap().push("tool".into());
            if self.fail {
                Err(ToolError::Execution("boom".into()))
            } else {
                Ok("done".into())
            }
        }
    }

    #[tokio::test]
    async fn a_wrapped_writer_syncs_before_and_after_and_a_failure_skips_the_push() {
        let dir = tempfile::tempdir().unwrap();
        let jail = crate::security::PathJail::new(dir.path());
        for (fail, expected) in [
            (false, vec!["before", "tool", "after PATH"]),
            (true, vec!["before", "tool"]),
        ] {
            let log = Arc::new(Recorder::default());
            let tool = SyncedTool::new(
                Arc::new(Inner {
                    log: log.clone(),
                    fail,
                }),
                log.clone(),
                jail.clone(),
                true,
            );
            let result = tool.execute(json!({"path": "src/main.rs"})).await;
            assert_eq!(result.is_err(), fail);
            let want: Vec<String> = expected
                .iter()
                .map(|s| {
                    s.replace(
                        "PATH",
                        &jail
                            .validate_path("src/main.rs")
                            .unwrap()
                            .display()
                            .to_string(),
                    )
                })
                .collect();
            assert_eq!(*log.calls.lock().unwrap(), want);
        }
    }

    #[tokio::test]
    async fn a_wrapped_reader_only_syncs_before() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Recorder::default());
        let tool = SyncedTool::new(
            Arc::new(Inner {
                log: log.clone(),
                fail: false,
            }),
            log.clone(),
            crate::security::PathJail::new(dir.path()),
            false,
        );
        tool.execute(json!({"path": "a"})).await.unwrap();
        assert_eq!(*log.calls.lock().unwrap(), ["before", "tool"]);
        assert_eq!(
            (tool.name(), tool.description()),
            ("write_file", "d"),
            "the wrapper is invisible to the model"
        );
    }

    // --- the host backend ----------------------------------------------------------------

    #[tokio::test]
    async fn the_host_backend_runs_a_local_command() {
        let backend = HostBackend::new(SandboxMode::Off);
        let dir = tempfile::tempdir().unwrap();
        let out = backend
            .exec(ExecRequest {
                command: "echo out; echo err >&2; exit 3".into(),
                cwd: dir.path().into(),
                timeout: Duration::from_secs(10),
            })
            .await
            .unwrap();
        assert_eq!(
            (out.stdout.as_str(), out.stderr.as_str(), out.exit_code),
            ("out\n", "err\n", 3)
        );
        assert_eq!(backend.label(), "host");
    }

    #[tokio::test]
    async fn the_host_backend_times_out_with_the_same_message() {
        let backend = HostBackend::new(SandboxMode::Off);
        let dir = tempfile::tempdir().unwrap();
        let e = backend
            .exec(ExecRequest {
                command: "sleep 5".into(),
                cwd: dir.path().into(),
                timeout: Duration::from_millis(200),
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("timed out"), "{e}");
    }
}
