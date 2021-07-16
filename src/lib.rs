use std::string::ToString;

use anyhow::Result;
use rand::prelude::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::sync::mpsc;

/// A 2D vector.
#[derive(Copy, Clone, Debug)]
pub struct Vec2(pub u32, pub u32);

/// A 24-bit RGB color.
#[derive(Copy, Clone, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl From<image::Rgb<u8>> for Rgb {
    fn from(image::Rgb([r, g, b]): image::Rgb<u8>) -> Self {
        Self(r, g, b)
    }
}

/// A Pixelflut command.
#[derive(Copy, Clone, Debug)]
pub enum Cmd {
    Help,
    Size,
    SetPx(Vec2, Rgb),
    GetPx(Vec2),
}

impl ToString for Cmd {
    fn to_string(&self) -> String {
        use Cmd::*;

        match *self {
            Help => "HELP".to_owned(),
            Size => "SIZE".to_owned(),
            SetPx(coordinate, rgb) => format!(
                "PX {} {} {:02x}{:02x}{:02x}",
                coordinate.0, coordinate.1, rgb.0, rgb.1, rgb.2
            ),
            GetPx(coordinate) => format!("PX {} {}", coordinate.0, coordinate.1),
        }
    }
}

/// A connection to a Pixelflut server.
pub struct Sock {
    inner: BufReader<TcpStream>,
}

impl Sock {
    /// Connects to a Pixelflut server.
    pub async fn connect<A>(addr: A) -> Result<Self>
    where
        A: ToSocketAddrs,
    {
        let stream = TcpStream::connect(addr).await?;
        let buf = BufReader::new(stream);

        Ok(Self { inner: buf })
    }

    /// Reads a line from the server.
    pub async fn read_line(&mut self) -> Result<String> {
        let mut response = String::new();
        self.inner.read_line(&mut response).await?;
        Ok(response)
    }

    /// Sends a command to the server.
    pub async fn send(&mut self, cmd: Cmd) -> Result<()> {
        let cmd = cmd.to_string() + "\n";
        self.inner.write_all(cmd.as_bytes()).await?;
        self.inner.flush().await?;
        Ok(())
    }

    /// Consumes this `Sock` in order to spawn a channel that is used to send commands to the inner socket.
    pub fn boot(mut self) -> mpsc::Sender<Cmd> {
        let (tx, mut rx): (mpsc::Sender<Cmd>, mpsc::Receiver<_>) = mpsc::channel(1024);

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                if let Err(err) = self.send(cmd).await {
                    eprintln!("failed to send cmd: {:?}", err);
                }
            }
        });

        tx
    }

    /// Queries the size of the canvas.
    pub async fn query_size(&mut self) -> Result<Vec2> {
        self.send(Cmd::Size).await?;
        let size_response = self.read_line().await?;

        let mut split = size_response.trim_end().splitn(3, ' ').skip(1);
        let width: u32 = split
            .next()
            .ok_or_else(|| anyhow::anyhow!("server gave no width in response to SIZE"))?
            .parse()?;
        let height: u32 = split
            .next()
            .ok_or_else(|| anyhow::anyhow!("server gave no height in response to SIZE"))?
            .parse()?;

        Ok(Vec2(width, height))
    }
}

/// Handles mass-sending pixels to Pixelflut servers.
pub struct Sender {
    pub sock: Sock,
    txs: Vec<mpsc::Sender<Cmd>>,
}

impl Sender {
    /// Connects to a Pixelflut server.
    pub async fn connect<A>(addr: A, n_sender_socks: usize) -> Result<Self>
    where
        A: ToSocketAddrs,
    {
        let mut txs = Vec::new();

        assert!(n_sender_socks > 0, "you must spawn at least one socket");

        for _ in 0..=n_sender_socks {
            let sock = Sock::connect(&addr).await?;
            txs.push(sock.boot());
        }

        Ok(Self {
            sock: Sock::connect(&addr).await?,
            txs,
        })
    }

    /// Pick a transmitter to use to interact with the server.
    fn pick_tx(&self) -> &mpsc::Sender<Cmd> {
        let mut rng = thread_rng();
        self.txs.choose(&mut rng).unwrap()
    }

    /// Enqueue a Pixelflut command to be sent to the server.
    pub async fn send(&self, cmd: Cmd) -> Result<()> {
        self.pick_tx().send(cmd).await?;
        Ok(())
    }
}
