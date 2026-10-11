//! Native line-delimited transport; the host applies requests in frame order.
use crate::{Request, Response};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
pub struct Pending {
    pub request: Request,
    pub reply: Sender<Response>,
}

/// Binds before spawning the listener, returning errors to the owning host.
pub fn listen(addr: &str) -> std::io::Result<Receiver<Pending>> {
    let listener = TcpListener::bind(addr)?;
    let (tx, rx) = channel();
    std::thread::spawn(move || accept_loop(listener, tx));
    Ok(rx)
}
fn accept_loop(listener: TcpListener, tx: Sender<Pending>) {
    for stream in listener.incoming().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || serve_connection(stream, tx));
    }
}

fn serve_connection(stream: TcpStream, tx: Sender<Pending>) {
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let response = match serde_json::from_str::<Request>(&line) {
            Err(e) => Response::err(0, format!("bad request: {e}")),
            Ok(request) => {
                let id = request.id;
                let (reply, rx) = channel();
                if tx.send(Pending { request, reply }).is_err() {
                    return;
                }
                rx.recv()
                    .unwrap_or_else(|_| Response::err(id, "game dropped the request"))
            }
        };
        let Ok(text) = serde_json::to_string(&response) else {
            return;
        };
        if writeln!(writer, "{text}").is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_preserves_order_ids_and_errors_on_dropped_replies() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let (stream, _) = listener.accept().unwrap();
        let (tx, rx) = channel();
        let worker = std::thread::spawn(move || serve_connection(stream, tx));
        let mut reader = BufReader::new(client.try_clone().unwrap());
        let read = |reader: &mut BufReader<TcpStream>| {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            serde_json::from_str::<Response>(&line).unwrap()
        };
        writeln!(client, "malformed").unwrap();
        let invalid = read(&mut reader);
        assert_eq!(invalid.id, 0);
        assert!(!invalid.ok);
        writeln!(client, "{{\"id\":41,\"cmd\":{{\"type\":\"ping\"}}}}").unwrap();
        writeln!(client, "{{\"id\":42,\"cmd\":{{\"type\":\"state\"}}}}").unwrap();
        let first = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(first.request.id, 41);
        assert_eq!(first.request.cmd, crate::Command::Ping);
        assert!(rx.try_recv().is_err());
        first
            .reply
            .send(Response::ok(41, serde_json::json!("pong")))
            .unwrap();
        let response = read(&mut reader);
        assert_eq!(response.id, 41);
        assert_eq!(response.result, Some(serde_json::json!("pong")));
        let second = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(second.request.id, 42);
        drop(second);
        let response = read(&mut reader);
        assert_eq!(response.id, 42);
        assert_eq!(response.error.as_deref(), Some("game dropped the request"));
        client.shutdown(std::net::Shutdown::Both).unwrap();
        worker.join().unwrap();
    }
}
