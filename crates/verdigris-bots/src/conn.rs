use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use verdigris_protocol::{
    build_compressed_packet, build_packet, decode_compressed_frame, decode_frame, read_var_int,
};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvError {
    Cancelled,
    Timeout,
    Reset,
}

pub struct Conn<S> {
    stream: S,
    read_buf: Vec<u8>,
    compression: Option<usize>,
    bytes_read: u64,
}

pub async fn connect(
    host: &str,
    port: u16,
    token: &CancellationToken,
) -> Result<Conn<TcpStream>, RecvError> {
    let attempt = timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)));
    tokio::select! {
        _ = token.cancelled() => Err(RecvError::Cancelled),
        result = attempt => match result {
            Ok(Ok(stream)) => {
                let _ = stream.set_nodelay(true);
                Ok(Conn::new(stream))
            }
            Ok(Err(_)) => Err(RecvError::Reset),
            Err(_) => Err(RecvError::Timeout),
        },
    }
}

impl<S> Conn<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new(stream: S) -> Conn<S> {
        Conn {
            stream,
            read_buf: Vec::with_capacity(8 * 1024),
            compression: None,
            bytes_read: 0,
        }
    }

    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    pub fn enable_compression(&mut self, threshold: usize) {
        self.compression = Some(threshold);
    }

    pub async fn send(
        &mut self,
        id: i32,
        payload: &[u8],
        token: &CancellationToken,
    ) -> Result<(), RecvError> {
        let frame = match self.compression {
            Some(threshold) => build_compressed_packet(id, payload, threshold),
            None => build_packet(id, payload),
        }
        .map_err(|_| RecvError::Reset)?;
        tokio::select! {
            _ = token.cancelled() => Err(RecvError::Cancelled),
            result = self.stream.write_all(&frame) => result.map_err(|_| RecvError::Reset),
        }
    }

    pub async fn recv(
        &mut self,
        read_timeout: Duration,
        token: &CancellationToken,
    ) -> Result<(i32, Vec<u8>), RecvError> {
        loop {
            if let Some((id, body)) = self.try_decode()? {
                return Ok((id, body));
            }
            let mut chunk = [0u8; 8 * 1024];
            let read = tokio::select! {
                _ = token.cancelled() => return Err(RecvError::Cancelled),
                result = timeout(read_timeout, self.stream.read(&mut chunk)) => result,
            };
            match read {
                Ok(Ok(0)) => return Err(RecvError::Reset),
                Ok(Ok(n)) => {
                    self.bytes_read += n as u64;
                    self.read_buf.extend_from_slice(&chunk[..n]);
                }
                Ok(Err(_)) => return Err(RecvError::Reset),
                Err(_) => return Err(RecvError::Timeout),
            }
        }
    }

    fn try_decode(&mut self) -> Result<Option<(i32, Vec<u8>)>, RecvError> {
        match self.compression {
            Some(threshold) => {
                match decode_compressed_frame(&self.read_buf, threshold)
                    .map_err(|_| RecvError::Reset)?
                {
                    Some((packet, consumed)) => {
                        let decoded = split_packet(&packet)?;
                        self.read_buf.drain(..consumed);
                        Ok(Some(decoded))
                    }
                    None => Ok(None),
                }
            }
            None => match decode_frame(&self.read_buf).map_err(|_| RecvError::Reset)? {
                Some((body, consumed)) => {
                    let decoded = split_packet(body)?;
                    self.read_buf.drain(..consumed);
                    Ok(Some(decoded))
                }
                None => Ok(None),
            },
        }
    }
}

fn split_packet(packet: &[u8]) -> Result<(i32, Vec<u8>), RecvError> {
    match read_var_int(packet).map_err(|_| RecvError::Reset)? {
        Some((id, header)) => Ok((id, packet[header..].to_vec())),
        None => Err(RecvError::Reset),
    }
}
