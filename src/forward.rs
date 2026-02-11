use std::io;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, Copy)]
pub struct PumpOptions {
    pub idle_timeout: Option<Duration>,
}

#[derive(Debug, Clone, Copy)]
pub enum CloseReason {
    SourceEof,
    DestinationEof,
    IdleTimeout,
}

#[derive(Debug, Clone, Copy)]
pub struct PumpStats {
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub reason: CloseReason,
    pub duration: Duration,
}

pub async fn pump(
    mut source: TcpStream,
    mut destination: TcpStream,
    opts: PumpOptions,
) -> io::Result<PumpStats> {
    let mut source_to_destination = 0_u64;
    let mut destination_to_source = 0_u64;
    let started = Instant::now();

    let mut source_buf = vec![0_u8; 16 * 1024];
    let mut destination_buf = vec![0_u8; 16 * 1024];

    loop {
        if let Some(timeout) = opts.idle_timeout {
            tokio::select! {
                source_read = source.read(&mut source_buf) => {
                    let count = source_read?;
                    if count == 0 {
                        let _ = destination.shutdown().await;
                        return Ok(PumpStats {
                            bytes_up: source_to_destination,
                            bytes_down: destination_to_source,
                            reason: CloseReason::SourceEof,
                            duration: started.elapsed(),
                        });
                    }
                    destination.write_all(&source_buf[..count]).await?;
                    source_to_destination += count as u64;
                }
                destination_read = destination.read(&mut destination_buf) => {
                    let count = destination_read?;
                    if count == 0 {
                        let _ = source.shutdown().await;
                        return Ok(PumpStats {
                            bytes_up: source_to_destination,
                            bytes_down: destination_to_source,
                            reason: CloseReason::DestinationEof,
                            duration: started.elapsed(),
                        });
                    }
                    source.write_all(&destination_buf[..count]).await?;
                    destination_to_source += count as u64;
                }
                _ = tokio::time::sleep(timeout) => {
                    let _ = source.shutdown().await;
                    let _ = destination.shutdown().await;
                    return Ok(PumpStats {
                        bytes_up: source_to_destination,
                        bytes_down: destination_to_source,
                        reason: CloseReason::IdleTimeout,
                        duration: started.elapsed(),
                    });
                }
            }
        } else {
            tokio::select! {
                source_read = source.read(&mut source_buf) => {
                    let count = source_read?;
                    if count == 0 {
                        let _ = destination.shutdown().await;
                        return Ok(PumpStats {
                            bytes_up: source_to_destination,
                            bytes_down: destination_to_source,
                            reason: CloseReason::SourceEof,
                            duration: started.elapsed(),
                        });
                    }
                    destination.write_all(&source_buf[..count]).await?;
                    source_to_destination += count as u64;
                }
                destination_read = destination.read(&mut destination_buf) => {
                    let count = destination_read?;
                    if count == 0 {
                        let _ = source.shutdown().await;
                        return Ok(PumpStats {
                            bytes_up: source_to_destination,
                            bytes_down: destination_to_source,
                            reason: CloseReason::DestinationEof,
                            duration: started.elapsed(),
                        });
                    }
                    source.write_all(&destination_buf[..count]).await?;
                    destination_to_source += count as u64;
                }
            }
        }
    }
}
