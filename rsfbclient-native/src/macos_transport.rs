//! Keep the peer of the native client's socket under our control. Firebird's
//! forced abort closes its own descriptor while a different thread is still
//! polling it. On macOS the blocked call can then fail to wake up.

use std::{
    io,
    net::{Ipv4Addr, Shutdown, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

pub(crate) struct Transport {
    port: u16,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    sockets: Option<(TcpStream, TcpStream)>,
    stopped: bool,
}

impl Transport {
    pub(crate) fn open(host: &str, port: u16) -> io::Result<Self> {
        let host = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host);
        let upstream = TcpStream::connect((host, port))?;
        upstream.set_nodelay(true)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let state = Arc::new(Mutex::new(State::default()));
        let worker_state = state.clone();
        thread::Builder::new()
            .name("firebird-tcp".into())
            .spawn(move || {
                let _ = relay(listener, upstream, &worker_state);
                let _ = stop(&worker_state);
            })?;
        Ok(Self { port, state })
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn shutdown(&self) -> io::Result<()> {
        stop(&self.state)
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn stop(state: &Mutex<State>) -> io::Result<()> {
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.stopped = true;
    if let Some((client, upstream)) = state.sockets.take() {
        // Close the server's peer as well: rollback must release server locks
        // even if the original query is still busy producing a result.
        let client_result = shutdown(&client, Shutdown::Both);
        let upstream_result = shutdown(&upstream, Shutdown::Both);
        client_result.and(upstream_result)
    } else {
        Ok(())
    }
}

fn shutdown(socket: &TcpStream, how: Shutdown) -> io::Result<()> {
    match socket.shutdown(how) {
        Err(error) if error.kind() == io::ErrorKind::NotConnected => Ok(()),
        result => result,
    }
}

fn relay(listener: TcpListener, mut upstream: TcpStream, state: &Mutex<State>) -> io::Result<()> {
    let mut client = loop {
        if state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stopped
        {
            return Ok(());
        }
        match listener.accept() {
            Ok((client, _)) => break client,
            // Only startup polls; the active transport uses blocking I/O.
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    };
    drop(listener);
    client.set_nonblocking(false)?;
    client.set_nodelay(true)?;
    let mut from_server = upstream.try_clone()?;
    let mut to_client = client.try_clone()?;
    {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped {
            return Ok(());
        }
        state.sockets = Some((client.try_clone()?, upstream.try_clone()?));
    }
    thread::scope(|scope| {
        thread::Builder::new()
            .name("firebird-tcp-reply".into())
            .spawn_scoped(scope, || forward(&mut from_server, &mut to_client))?;
        forward(&mut client, &mut upstream);
        Ok(())
    })
}

fn forward(from: &mut TcpStream, to: &mut TcpStream) {
    if io::copy(from, to).is_ok() {
        let _ = shutdown(to, Shutdown::Write);
    } else {
        let _ = shutdown(from, Shutdown::Both);
        let _ = shutdown(to, Shutdown::Both);
    }
}
