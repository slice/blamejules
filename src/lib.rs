use std::string::ToString;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, ToSocketAddrs};

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

/// A pixelflut command.
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

/// Handles sending pixelflut pixels.
pub struct Sender {
    /// The buffered TCP stream representing the connection we have to the server.
    sock: BufReader<TcpStream>,
}

impl Sender {
    /// Connects to a pixelflut server.
    pub async fn connect<A>(addr: A) -> Result<Self>
    where
        A: ToSocketAddrs,
    {
        let stream = TcpStream::connect(addr).await?;
        let buf = BufReader::new(stream);

        Ok(Self { sock: buf })
    }

    /// Sends a pixelflut command to the server.
    pub async fn send(&mut self, cmd: Cmd) -> Result<()> {
        let cmd = cmd.to_string() + "\n";
        self.sock.write_all(cmd.as_bytes()).await?;
        self.sock.flush().await?;
        Ok(())
    }

    /// Queries the size of the canvas.
    pub async fn query_size(&mut self) -> Result<Vec2> {
        self.send(Cmd::Size).await?;

        let mut size_response = String::new();
        self.sock.read_line(&mut size_response).await?;

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
