//! Child process owned by the Qt frontend. Standard output is JSON lines only.
mod adapter;
mod keymap;
mod terminal;
mod workspace;

use adapter::{Adapter, VERSION};
use anyhow::Result;
use clap::Parser;
use rex_rune::{
    connection::{HostConnection, wait_for_hosts},
    transport,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version, about = "Private backend for the Rex Qt Quick frontend")]
struct Options {
    #[arg(long, default_value_t = transport::local_endpoint())]
    endpoint: String,
    #[arg(long)]
    session: Option<String>,
    #[arg(long, default_value_os_t = config_path())]
    config: PathBuf,
}

fn config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".config")
        })
        .join("rex-qt/workspace.json")
}

fn main() -> Result<()> {
    let options = Options::parse();
    let mut adapter = Adapter::default();
    adapter.configure(options.config)?;
    adapter.endpoint = options.endpoint.clone();
    let host = HostConnection::spawn_labeled(
        options.endpoint,
        options.session,
        None,
        None,
        true,
        None,
        "Rex Qt Quick".into(),
    );
    adapter.declare_keymap(&host)?;
    let (send, receive) = flume::bounded(64);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut publish = |value: &Value| -> Result<()> {
        serde_json::to_writer(&mut output, value)?;
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    };
    publish(&json!({"type":"hello","version":VERSION}))?;
    let mut queued_actions = Vec::new();
    let mut previous = Value::Null;
    let mut dirty = true;
    let mut next_frame = Instant::now();
    loop {
        match receive.recv_timeout(Duration::from_millis(33)) {
            Ok(line) => {
                let result = line.map_err(anyhow::Error::from).and_then(|line| {
                    let request = serde_json::from_str(&line)?;
                    adapter.handle(&host, &request)
                });
                match result {
                    Ok(false) => break,
                    Ok(true) => dirty = true,
                    Err(error) => publish(
                        &json!({"type":"error","version":VERSION,"message":error.to_string()}),
                    )?,
                }
            }
            Err(flume::RecvTimeoutError::Disconnected) => break,
            Err(flume::RecvTimeoutError::Timeout) => {}
        }
        dirty |= host.wake.drain().next().is_some();
        {
            let mut model = host.model.lock().expect("host model");
            queued_actions.extend(adapter.take_actions(&mut model));
        }
        dirty |= !queued_actions.is_empty();
        let clipboard = {
            let mut model = host.model.lock().expect("host model");
            model
                .clipboard
                .take()
                .filter(|copy| {
                    rex_rune::connection::clipboard::accepts(&model, &copy.session, &copy.block)
                })
                .map(|copy| {
                    json!({"type":"clipboard","version":VERSION,"epoch":model.connection_epoch,
                    "session":copy.session,"pane":copy.block,"text":copy.text})
                })
        };
        if let Some(copy) = clipboard {
            publish(&copy)?;
        }
        let notice = {
            let mut model = host.model.lock().expect("host model");
            for connection in model.terminals.values() {
                dirty |= connection.wake.drain().next().is_some();
            }
            model.notice.take()
        };
        if let Some(message) = notice {
            publish(&json!({"type":"error","version":VERSION,"message":message}))?;
        }
        if dirty && Instant::now() >= next_frame {
            let model = host.model.lock().expect("host model").clone();
            let next = adapter.state(&model);
            if next != previous {
                publish(&next)?;
                previous = next;
            }
            for action in queued_actions.drain(..) {
                if action["epoch"].as_u64() == Some(model.connection_epoch)
                    && action["generation"].as_u64() == Some(adapter.keymap_generation)
                {
                    publish(&action)?;
                }
            }
            dirty = false;
            next_frame = Instant::now() + Duration::from_millis(33);
        }
    }
    drop(host);
    wait_for_hosts(Duration::from_secs(3));
    Ok(())
}
