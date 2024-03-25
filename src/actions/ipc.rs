//! A very simple IPC server for communicating with the build container.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{BufRead, Read, Write},
    net::Shutdown,
    os::unix::net::UnixListener,
    path::PathBuf,
};

use crate::{error, machine::terminate_container_by_name, repo::refresh_repo};
use console::style;

use super::{rollback_container, run_in_container};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
enum IpcCommand {
    Refresh,
    Reboot { rollback: bool },
    Abort { reason: String },
}

#[derive(Debug, Serialize, Deserialize)]
struct IpcProtocol {
    jsonrpc: String,
    id: usize,
    #[serde(flatten)]
    cmd: IpcCommand,
}

#[derive(Debug, Serialize, Deserialize)]
struct IpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct IpcResponse {
    jsonrpc: String,
    id: usize,
    result: Option<Value>,
    error: Option<IpcError>,
}

impl IpcResponse {
    fn new_from_request(req: &IpcProtocol) -> Self {
        Self {
            jsonrpc: req.jsonrpc.clone(),
            id: req.id,
            result: None,
            error: None,
        }
    }
}

pub struct IpcServer {
    listener: UnixListener,
    instance: String,
    output_dir: PathBuf,
    location: String,
}

impl IpcServer {
    pub fn new(instance: String, output_dir: PathBuf) -> Result<Self> {
        let location = format!("{}/.ciel-ipc.sock", instance);
        let listener = UnixListener::bind(&location)?;
        Ok(Self {
            listener,
            instance,
            output_dir,
            location,
        })
    }

    pub fn get_sock_location(&self) -> &str {
        return &self.location;
    }

    pub fn spawn(&self) -> Result<()> {
        loop {
            match self.listener.accept() {
                Ok((socket, _)) => {
                    let mut bufreader = std::io::BufReader::new(socket);
                    let mut buf = String::with_capacity(1024);
                    bufreader.read_line(&mut buf)?;
                    if buf.starts_with("Content-Length:") {
                        let content_length: usize = buf
                            .split_whitespace()
                            .nth(1)
                            .ok_or_else(|| anyhow!("Invalid Content-Length header"))?
                            .parse()?;
                        if content_length >= 1024 * 1024 {
                            error!("Content too large {} bytes", content_length);
                            bufreader.into_inner().shutdown(Shutdown::Both).ok();
                            continue;
                        }
                        bufreader.read_line(&mut buf).ok(); // skip the next newline
                        let mut buf = vec![0; content_length];
                        bufreader.read(&mut buf)?;
                        let req: IpcProtocol = serde_json::from_slice(&buf)?;
                        let resp = self.handle_request(req)?;
                        let resp = serde_json::to_string(&resp)?;
                        let resp = format!("Content-Length: {}\r\n\r\n{}", resp.len(), resp);
                        let mut stream = bufreader.into_inner();
                        stream.write_all(resp.as_bytes())?;
                        continue;
                    }
                    error!("Invalid request header: {}", buf);
                    bufreader.into_inner().shutdown(Shutdown::Both).ok();
                }
                Err(_) => return Err(anyhow!("IpcServer error")),
            }
        }
    }

    fn handle_request(&self, req: IpcProtocol) -> Result<IpcResponse> {
        let mut resp = IpcResponse::new_from_request(&req);
        match req.cmd {
            IpcCommand::Refresh => match refresh_repo(&self.output_dir) {
                Ok(()) => {
                    resp.result = Some(Value::Null);
                }
                Err(e) => {
                    resp.error = Some(IpcError {
                        code: -32803,
                        message: e.to_string(),
                    });
                }
            },
            IpcCommand::Reboot { rollback } => {
                // actually the application inside the container will never receive this response
                // because it will be terminated upon reboot
                resp.result = Some(Value::Null);
                if rollback {
                    rollback_container(&self.instance)?;
                } else {
                    run_in_container(&self.instance, &["reboot"])?;
                }
            }
            IpcCommand::Abort { reason } => {
                error!("container reported error: {}", reason);
                terminate_container_by_name(&self.instance)?;
                bail!("aborted due to fatal error");
            }
        }

        Ok(resp)
    }
}

#[test]
fn test_ipc_protocol() {
    let cmd = IpcProtocol {
        jsonrpc: "2.0".to_string(),
        id: 1,
        cmd: IpcCommand::Refresh,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    assert_eq!(
        json,
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"Refresh\"}"
    );
}
