//! Child process owned by the Qt frontend. Standard output is JSON lines only.
use anyhow::Result;
use clap::Parser;
use rex_rune::{
    connection::{HostConnection, wait_for_hosts},
    qt_backend::{Adapter, VERSION},
    transport,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version, about = "Private backend for the Rex Qt Quick frontend")]
struct Options {
    #[arg(long, default_value_t = transport::local_endpoint())]
    endpoint: String,
    #[arg(long)]
    session: Option<String>,
}

fn main() -> Result<()> {
    let options = Options::parse();
    let host = HostConnection::spawn_labeled(
        options.endpoint,
        options.session,
        None,
        None,
        true,
        None,
        "Rex Qt Quick".into(),
    );
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
    let mut adapter = Adapter::default();
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
            dirty = false;
            next_frame = Instant::now() + Duration::from_millis(33);
        }
    }
    drop(host);
    wait_for_hosts(Duration::from_secs(3));
    Ok(())
}
