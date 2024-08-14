use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
};

pub struct UnixSocket {
    listener: UnixListener,
}

impl UnixSocket {
    pub fn create<T: AsRef<Path>>(path: T) -> std::io::Result<Self> {
        let path = path.as_ref();
        // TODO: Find a way to use sockets that works with more than 1 instance.
        let _ = std::fs::remove_file(path);
        let sock = UnixListener::bind(path)?;

        Ok(Self { listener: sock })
    }

    pub fn accept(&self) -> std::io::Result<UnixClient> {
        Ok(self.listener.accept()?.0.into())
    }
}

pub struct UnixClient {
    stream: UnixStream,
}

impl UnixClient {
    /// Block until a read is available and return all contents as a string.
    ///
    /// NOTE: currently only reads at most 1024 bytes.
    pub fn read_string(&mut self) -> std::io::Result<String> {
        // TODO: Allow reading more than 1024 bytes.
        let mut buf = [0u8; 1024];
        let len = self.stream.read(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf[..len]).to_string())
    }

    pub fn send_message(&mut self, message: &str) -> std::io::Result<()> {
        self.stream.write_all(format!("{message}\n").as_bytes())
    }
}

impl From<UnixStream> for UnixClient {
    fn from(value: UnixStream) -> Self {
        Self { stream: value }
    }
}
