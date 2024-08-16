use std::{
    io::{BufRead, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
};

use bufstream::BufStream;

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
    stream: BufStream<UnixStream>,
}

impl UnixClient {
    /// Block until a read is available and return all contents as a string.
    pub fn read_line(&mut self) -> std::io::Result<String> {
        let mut out = String::new();
        self.stream.read_line(&mut out)?;
        Ok(out)
    }

    pub fn send_message(&mut self, message: &str) -> std::io::Result<()> {
        self.stream.write_all(format!("{message}\n").as_bytes())
    }
}

impl From<UnixStream> for UnixClient {
    fn from(value: UnixStream) -> Self {
        Self {
            stream: BufStream::new(value),
        }
    }
}
