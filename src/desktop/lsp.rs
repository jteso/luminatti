//! Minimal local TypeScript LSP client used by the native review workspace.
//!
//! The client talks JSON-RPC over stdio to `vtsls` or
//! `typescript-language-server`. It is deliberately read-only and only opens
//! the currently inspected TypeScript document.

use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LspStatus {
    Idle,
    Starting,
    Ready(String),
    Stopped,
    Unavailable,
    Failed(String),
}

impl LspStatus {
    pub fn label(&self) -> String {
        match self {
            Self::Idle => "Open a TypeScript file to start".into(),
            Self::Starting => "Starting TypeScript language server".into(),
            Self::Ready(name) => format!("{name} running"),
            Self::Stopped => "TypeScript language server stopped".into(),
            Self::Unavailable => "TypeScript language server not installed".into(),
            Self::Failed(error) => format!("Language server failed: {error}"),
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspSymbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
    pub end_line: usize,
    pub depth: usize,
}

#[derive(Clone, Debug)]
pub struct ReferenceTarget {
    pub key: String,
    pub line: usize,
    pub column: usize,
}

pub enum LspCommand {
    Analyze {
        path: PathBuf,
        source: String,
        targets: Vec<ReferenceTarget>,
    },
    Restart,
    Stop,
}

pub enum LspEvent {
    Status(LspStatus),
    Analysis {
        path: PathBuf,
        symbols: Vec<LspSymbol>,
        references: HashMap<String, usize>,
    },
}

pub fn spawn(root: PathBuf) -> (Sender<LspCommand>, Receiver<LspEvent>) {
    let (command_sender, command_receiver) = mpsc::channel();
    let (event_sender, event_receiver) = mpsc::channel();
    std::thread::spawn(move || worker(root, command_receiver, event_sender));
    (command_sender, event_receiver)
}

fn worker(root: PathBuf, commands: Receiver<LspCommand>, events: Sender<LspEvent>) {
    let mut server: Option<Server> = None;
    let mut explicitly_stopped = false;
    for command in commands {
        match command {
            LspCommand::Stop => {
                explicitly_stopped = true;
                if let Some(mut running) = server.take() {
                    running.stop();
                }
                let _ = events.send(LspEvent::Status(LspStatus::Stopped));
            }
            LspCommand::Restart => {
                explicitly_stopped = false;
                if let Some(mut running) = server.take() {
                    running.stop();
                }
                server = start_server(&root, &events);
            }
            LspCommand::Analyze {
                path,
                source,
                targets,
            } => {
                if server.is_none() && !explicitly_stopped {
                    server = start_server(&root, &events);
                }
                let Some(running) = server.as_mut() else {
                    continue;
                };
                match running.analyze(&path, &source, &targets) {
                    Ok((symbols, references)) => {
                        let _ = events.send(LspEvent::Analysis {
                            path,
                            symbols,
                            references,
                        });
                    }
                    Err(error) => {
                        let message = error.to_string();
                        running.stop();
                        server = None;
                        let _ = events.send(LspEvent::Status(LspStatus::Failed(message)));
                    }
                }
            }
        }
    }
    if let Some(mut running) = server {
        running.stop();
    }
}

fn start_server(root: &Path, events: &Sender<LspEvent>) -> Option<Server> {
    let _ = events.send(LspEvent::Status(LspStatus::Starting));
    let Some((name, executable)) = discover_server(root) else {
        let _ = events.send(LspEvent::Status(LspStatus::Unavailable));
        return None;
    };
    match Server::start(root, &name, &executable) {
        Ok(server) => {
            let _ = events.send(LspEvent::Status(LspStatus::Ready(name)));
            Some(server)
        }
        Err(error) => {
            let _ = events.send(LspEvent::Status(LspStatus::Failed(error.to_string())));
            None
        }
    }
}

fn discover_server(root: &Path) -> Option<(String, PathBuf)> {
    // Prefer the workspace's pinned language-server version. GUI applications
    // often inherit a reduced PATH, and a local binary also keeps the result
    // reproducible for commercial projects.
    let local_bin = root.join("node_modules").join(".bin");
    for name in ["vtsls", "typescript-language-server"] {
        let candidate = local_bin.join(name);
        if candidate.is_file() {
            return Some((name.to_string(), candidate));
        }
    }

    let path = env::var_os("PATH")?;
    for name in ["vtsls", "typescript-language-server"] {
        for directory in env::split_paths(&path) {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some((name.to_string(), candidate));
            }
        }
    }
    None
}

struct Server {
    child: Child,
    input: ChildStdin,
    output: Receiver<io::Result<Value>>,
    next_id: u64,
    open_versions: HashMap<String, i64>,
}

impl Server {
    fn start(root: &Path, _name: &str, executable: &Path) -> io::Result<Self> {
        let mut child = Command::new(executable)
            .arg("--stdio")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("language server stdin unavailable"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("language server stdout unavailable"))?;
        let (output_sender, output_receiver) = mpsc::channel();
        std::thread::spawn(move || read_server_output(output, output_sender));
        let mut server = Self {
            child,
            input,
            output: output_receiver,
            next_id: 1,
            open_versions: HashMap::new(),
        };
        let root_uri = path_uri(root)?;
        let response = server.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "workspaceFolders": [{"uri": root_uri, "name": root.file_name().and_then(|v| v.to_str()).unwrap_or("workspace")}],
                "capabilities": {
                    "textDocument": {
                        "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                        "references": {"dynamicRegistration": false}
                    },
                    "workspace": {"configuration": true}
                },
                "clientInfo": {"name": "Luminatti", "version": env!("CARGO_PKG_VERSION")}
            }),
        )?;
        if response.get("error").is_some() {
            return Err(io::Error::other(format!(
                "initialize rejected: {}",
                response["error"]
            )));
        }
        server.notify("initialized", json!({}))?;
        Ok(server)
    }

    fn analyze(
        &mut self,
        path: &Path,
        source: &str,
        targets: &[ReferenceTarget],
    ) -> io::Result<(Vec<LspSymbol>, HashMap<String, usize>)> {
        let uri = path_uri(path)?;
        let version = {
            let version = self.open_versions.entry(uri.clone()).or_insert(0);
            *version += 1;
            *version
        };
        let language_id = if path.extension().and_then(|value| value.to_str()) == Some("tsx") {
            "typescriptreact"
        } else {
            "typescript"
        };
        if version == 1 {
            self.notify(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": language_id, "version": version, "text": source}}),
            )?;
        } else {
            self.notify(
                "textDocument/didChange",
                json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": source}]}),
            )?;
        }

        let symbols_response = self.request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        )?;
        let mut symbols = Vec::new();
        flatten_symbols(
            symbols_response.get("result").unwrap_or(&Value::Null),
            0,
            &mut symbols,
        );

        let mut references = HashMap::new();
        for target in targets.iter().take(16) {
            let response = self.request(
                "textDocument/references",
                json!({
                    "textDocument": {"uri": uri},
                    "position": {"line": target.line.saturating_sub(1), "character": target.column},
                    "context": {"includeDeclaration": false}
                }),
            )?;
            let count = response["result"].as_array().map(Vec::len).unwrap_or(0);
            references.insert(target.key.clone(), count);
        }
        Ok((symbols, references))
    }

    fn request(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        write_message(
            &mut self.input,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )?;
        loop {
            let message = self
                .output
                .recv_timeout(Duration::from_secs(12))
                .map_err(|error| io::Error::new(io::ErrorKind::TimedOut, error))??;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                return Ok(message);
            }
            // Language servers may ask for configuration or capability setup
            // while a request is pending. A neutral response keeps the client
            // protocol-correct without granting mutation capabilities.
            if let Some(method) = message.get("method").and_then(Value::as_str) {
                if let Some(request_id) = message.get("id") {
                    let result = if method == "workspace/configuration" {
                        let count = message
                            .pointer("/params/items")
                            .and_then(Value::as_array)
                            .map(Vec::len)
                            .unwrap_or(0);
                        Value::Array((0..count).map(|_| json!({})).collect())
                    } else {
                        Value::Null
                    };
                    write_message(
                        &mut self.input,
                        &json!({"jsonrpc": "2.0", "id": request_id, "result": result}),
                    )?;
                }
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> io::Result<()> {
        write_message(
            &mut self.input,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        )
    }

    fn stop(&mut self) {
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_server_output(output: ChildStdout, sender: Sender<io::Result<Value>>) {
    let mut reader = BufReader::new(output);
    loop {
        let message = read_message(&mut reader);
        let done = message.is_err();
        if sender.send(message).is_err() || done {
            break;
        }
    }
}

fn path_uri(path: &Path) -> io::Result<String> {
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|_| io::Error::other(format!("cannot convert {} to a file URI", path.display())))
}

fn write_message(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "language server closed stdout",
            ));
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some(value) = header
            .strip_prefix("Content-Length:")
            .and_then(|value| value.trim().parse::<usize>().ok())
        {
            content_length = Some(value);
        }
    }
    let length = content_length.ok_or_else(|| io::Error::other("missing Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(io::Error::other)
}

fn flatten_symbols(value: &Value, depth: usize, symbols: &mut Vec<LspSymbol>) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        // A selectionRange covers only the name. Folding needs the complete
        // declaration, including its body and closing line.
        let range = item.get("range").or_else(|| item.get("selectionRange"));
        let line = range
            .and_then(|range| range.pointer("/start/line"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize
            + 1;
        let end_line = range
            .and_then(|range| range.pointer("/end/line"))
            .and_then(Value::as_u64)
            .unwrap_or(line.saturating_sub(1) as u64) as usize
            + 1;
        if let Some(name) = item.get("name").and_then(Value::as_str) {
            symbols.push(LspSymbol {
                name: name.to_string(),
                kind: symbol_kind_label(item.get("kind").and_then(Value::as_u64)),
                line,
                end_line,
                depth,
            });
        }
        flatten_symbols(&item["children"], depth + 1, symbols);
    }
}

fn symbol_kind_label(kind: Option<u64>) -> String {
    match kind {
        Some(5) => "class",
        Some(6) => "method",
        Some(10) => "enum",
        Some(11) => "interface",
        Some(12) => "function",
        Some(13) => "variable",
        Some(14) => "constant",
        Some(22) => "enum member",
        Some(23) => "struct",
        Some(26) => "type parameter",
        _ => "symbol",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn discovers_workspace_language_server_before_path() {
        let fixture = tempfile::tempdir().unwrap();
        let local_bin = fixture.path().join("node_modules").join(".bin");
        std::fs::create_dir_all(&local_bin).unwrap();
        let vtsls = local_bin.join("vtsls");
        std::fs::write(&vtsls, "#!/bin/sh\n").unwrap();

        let discovered = discover_server(fixture.path()).unwrap();
        assert_eq!(discovered.0, "vtsls");
        assert_eq!(discovered.1, vtsls);
    }

    #[test]
    fn reads_lsp_content_length_frames() {
        let body = r#"{"jsonrpc":"2.0","id":4,"result":null}"#;
        let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let value = read_message(&mut Cursor::new(input)).unwrap();
        assert_eq!(value["id"], 4);
    }

    #[test]
    fn flattens_hierarchical_document_symbols() {
        let value = json!([{
            "name": "Cart",
            "kind": 5,
            "selectionRange": {"start": {"line": 3}, "end": {"line": 9}},
            "children": [{
                "name": "total",
                "kind": 6,
                "selectionRange": {"start": {"line": 5}, "end": {"line": 7}}
            }]
        }]);
        let mut symbols = Vec::new();
        flatten_symbols(&value, 0, &mut symbols);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].line, 4);
        assert_eq!(symbols[1].depth, 1);
        assert_eq!(symbols[1].kind, "method");
    }
}
