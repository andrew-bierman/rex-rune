use std::process::{Child, Command, Stdio};

pub(crate) const LINKS_SUPPORTED: bool = cfg!(target_os = "linux");

#[derive(Default)]
pub(crate) struct LinkOpener(Vec<Child>);

impl LinkOpener {
    pub(crate) fn reap(&mut self) -> bool {
        let mut failed = false;
        self.0.retain_mut(|child| match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) if status.success() => false,
            _ => {
                failed = true;
                false
            }
        });
        failed
    }

    pub(crate) fn open(&mut self, uri: &str) -> Result<(), &'static str> {
        if !LINKS_SUPPORTED || !crate::pane_links::web_url(uri) {
            return Err("Only HTTP and HTTPS pane links can be opened on Linux");
        }
        self.reap();
        if self.0.len() == 4 {
            return Err("The desktop link opener is busy");
        }
        let child = Command::new("xdg-open")
            .arg(uri)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Could not start the desktop link opener")?;
        self.0.push(child);
        Ok(())
    }
}
